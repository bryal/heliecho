#![feature(slice_patterns)]

extern crate pulse_simple;
extern crate rustfft;
extern crate num;
extern crate serial;
extern crate time;

use std::{io, thread, ops};
use std::io::prelude::*;
use std::sync::mpsc;
use pulse_simple::Record;
use num::Complex;
use serial::prelude::*;

const SAMPLE_RATE: usize = 48000;
// Must be power of 2
const SAMPLES_PER_PERIOD: usize = 1024;

const BASS_CUTOFF: f32 = 410.0;
const HIGH_CUTOFF: f32 = 3800.0;

fn bin_to_freq(i: usize) -> f32 {
    (i * SAMPLE_RATE) as f32 / SAMPLES_PER_PERIOD as f32
}

fn freq_to_bin(f: f32) -> usize {
    (f * SAMPLES_PER_PERIOD as f32 / SAMPLE_RATE as f32 + 0.5) as usize
}

const MAX_FREQ: f32 = 20_000.0;

fn pass_to(freq: f32, amp: f32, cut: f32) -> f32 {
    assert!(freq >= 0.0);

    if freq > cut {
        0.0
    } else {
        let x = freq / cut;
        let sharpness = (cut / 400.0).powf(2.0);

        amp * f32::max(1.0 - (3.0 * sharpness).powf(7.0 * (x - 1.0)), 0.0)
    }
}

fn band_pass(freq: f32, amp: f32, low_cut: f32, high_cut: f32) -> f32 {
    pass_from(freq, pass_to(freq, amp, high_cut), low_cut)
}

fn pass_from(freq: f32, amp: f32, cut: f32) -> f32 {
    assert!(freq >= 0.0 && cut < MAX_FREQ);

    if freq < cut {
        0.0
    } else {
        let x = (freq - cut) / (MAX_FREQ - cut);
        let sharpness = ((MAX_FREQ - cut) / 2700.0).powf(10.0);

        amp * f32::max(1.0 - (3.0 * sharpness).powf(-7.0 * x), 0.0)
    }
}

fn bass_pass(freq: f32, amp: f32) -> f32 {
    pass_to(freq, amp, BASS_CUTOFF)
}

fn mid_pass(freq: f32, amp: f32) -> f32 {
    band_pass(freq, amp, BASS_CUTOFF - 170.0, HIGH_CUTOFF + 300.0)
}

fn high_pass(freq: f32, amp: f32) -> f32 {
    pass_from(freq, amp, HIGH_CUTOFF)
}

/// Normalize a decibel value to [0, 1]
fn norm_db(db: f32) -> f32 {
    let x = db / 52.0;

    if x > 1.0 {
        1.0
    } else {
        (1.0 - x) * x + x * (1.0 / (1.0 + (-10.0 * (x - 0.5)).exp()))
    }
}

/// bin == index of fft where there is a corresponding frequence
fn stereo_pcm_to_db_bins(stereo_data: &[[f32; 2]; SAMPLES_PER_PERIOD])
                         -> [f32; SAMPLES_PER_PERIOD >> 1] {
    let complex_zero = Complex {
        re: 0.0f32,
        im: 0.0f32,
    };

    let mut avg_data = [complex_zero; SAMPLES_PER_PERIOD];

    for (i, &[l, r]) in stereo_data.iter().enumerate() {
        avg_data[i] = Complex {
            re: (l + r) / 2.0,
            im: 0.0,
        };
    }

    let mut fft = rustfft::FFT::new(SAMPLES_PER_PERIOD, false);

    let mut bin_amps_complex = [complex_zero; SAMPLES_PER_PERIOD];

    fft.process(&avg_data as &[_], &mut bin_amps_complex as &mut [_]);

    let mut bin_amps_db = [0.0f32; SAMPLES_PER_PERIOD >> 1];

    for (i, &c) in bin_amps_complex.iter().take(SAMPLES_PER_PERIOD >> 1).enumerate() {
        let amp = (c.re.powi(2) + c.im.powi(2)).sqrt();
        bin_amps_db[i] = 20.0 * amp.log(10.0);
    }

    bin_amps_db
}

fn max_amp(amps: &[f32]) -> (usize, f32) {
    let (mut max_bin, mut max_amp) = (0, 0.0);

    for i in 0..amps.len() {
        if amps[i] > max_amp {
            max_amp = amps[i];
            max_bin = i;
        }
    }

    (max_bin, max_amp)
}

const ADALIGHT_BAUDRATE: serial::BaudRate = serial::Baud115200;

const N_LEDS: usize = 50;

#[derive(Clone, Copy)]
struct Rgb8 {
    r: u8,
    g: u8,
    b: u8,
}

impl Rgb8 {
    fn brightness(self, f: f32) -> Rgb8 {
        Rgb8 {
            r: (self.r as f32 * f) as u8,
            g: (self.g as f32 * f) as u8,
            b: (self.b as f32 * f) as u8,
        }
    }
}

impl ops::Add for Rgb8 {
    type Output = Rgb8;

    fn add(self, rhs: Rgb8) -> Rgb8 {
        Rgb8 {
            r: self.r + rhs.r,
            g: self.g + rhs.g,
            b: self.b + rhs.b,
        }
    }
}

/// Go faster towards light, slower towards dark
fn smooth_color(from: Rgb8, to: Rgb8) -> Rgb8 {
    let from_tot = from.r as u16 + from.g as u16 + from.b as u16;
    let to_tot = to.r as u16 + to.g as u16 + to.b as u16;

    if to_tot > from_tot {
        from.brightness(0.4) + to.brightness(0.6)
    } else {
        from.brightness(0.6) + to.brightness(0.4)
    }
}

/// Initialize a thread for serial writing given a serial port, baud rate, header to write before
/// each data write, and buffer with the actual led color data.
fn init_write_thread(port: &str) -> mpsc::SyncSender<Rgb8> {
    use std::io::Write;

    let mut serial_con = serial::open(port).unwrap();

    serial_con.reconfigure(&|cfg| cfg.set_baud_rate(ADALIGHT_BAUDRATE))
        .unwrap();

    let (tx, rx) = mpsc::sync_channel::<Rgb8>(0);

    thread::spawn(move || {
        let count_high = ((N_LEDS - 1) >> 8) as u8;  // LED count high byte
        let count_low = ((N_LEDS - 1) & 0xff) as u8; // LED count low byte

        let mut color_buf = [0; 6 + 3 * N_LEDS];

        // Header
        color_buf[0] = 'A' as u8;
        color_buf[1] = 'd' as u8;
        color_buf[2] = 'a' as u8;
        color_buf[3] = count_high;
        color_buf[4] = count_low;
        color_buf[5] = count_high ^ count_low ^ 0x55; // Checksum

        let mut prev_recv_color = Rgb8 { r: 0, g: 0, b: 0 };
        let mut prev_color = Rgb8 { r: 0, g: 0, b: 0 };

        loop {
            let recv_color = rx.try_recv().unwrap_or(prev_color);

            let color = smooth_color(prev_color, recv_color);

            for n in 0..N_LEDS {
                color_buf[6 + 3 * n] = color.r;
                color_buf[6 + 3 * n + 1] = color.g;
                color_buf[6 + 3 * n + 2] = color.b;
            }

            match serial_con.write(&color_buf[..]) {
                Ok(bn) if bn == color_buf.len() => (),
                Ok(_) => println!("Failed to write all bytes of RGB data"),
                Err(e) => println!("Failed to write RGB data, {}", e),
            }

            prev_recv_color = recv_color;
            prev_color = color;
        }
    });

    tx
}

fn main() {
    // Do serial writing on own thread as to not block.
    let write_thread_tx = init_write_thread("/dev/ttyUSB0");

    let recorder = Record::new("heliecho",
                               "Capture audio to stream as color data to adalight device",
                               None,
                               SAMPLE_RATE as u32);

    let mut stereo_data = [[0.0f32; 2]; SAMPLES_PER_PERIOD];

    let (bass_cutoff_bin, high_cutoff_bin) = (freq_to_bin(BASS_CUTOFF), freq_to_bin(HIGH_CUTOFF));

    println!("");

    loop {
        // Record
        recorder.read(&mut stereo_data);

        let bin_amps_db = stereo_pcm_to_db_bins(&stereo_data);
        let mut bass_amps_db = [0.0; SAMPLES_PER_PERIOD >> 1];
        for (bin, &db) in bin_amps_db.iter().enumerate() {
            bass_amps_db[bin] = bass_pass(bin_to_freq(bin), db);
        }

        let mut mid_amps_db = [0.0; SAMPLES_PER_PERIOD >> 1];
        for (bin, &db) in bin_amps_db.iter().enumerate() {
            mid_amps_db[bin] = mid_pass(bin_to_freq(bin), db);
        }

        let mut high_amps_db = [0.0; SAMPLES_PER_PERIOD >> 1];
        for (bin, &db) in bin_amps_db.iter().enumerate() {
            high_amps_db[bin] = high_pass(bin_to_freq(bin), db);
        }

        let (max_bin_all, max_amp_all) = max_amp(&bin_amps_db);
        let (max_bin_bass, max_amp_bass) = max_amp(&bass_amps_db[..bass_cutoff_bin]);
        let m_m = max_amp(&mid_amps_db[bass_cutoff_bin..high_cutoff_bin]);
        let m_h = max_amp(&high_amps_db[high_cutoff_bin..]);

        let (max_bin_mid, max_amp_mid) = (m_m.0 + bass_cutoff_bin, m_m.1);
        let (max_bin_high, max_amp_high) = (m_h.0 + high_cutoff_bin, m_h.1);

        print!("\rf: {:6.0}, db: {:4.1}, vol: {:1.3}",
               bin_to_freq(max_bin_all),
               max_amp_all,
               norm_db(max_amp_all));
        io::stdout().flush().unwrap();

        let (bass_lvl, mid_lvl, high_lvl) =
            (norm_db(max_amp_bass), norm_db(max_amp_mid), norm_db(max_amp_high));

        let brightness = (bass_lvl * 1.4 + mid_lvl * 0.9 + high_lvl * 0.7) / 3.0;

        let color = Rgb8 {
                r: (255.0 * bass_lvl.powf(2.5) + 0.5) as u8,
                g: (255.0 * mid_lvl.powf(2.6) + 0.5) as u8,
                b: (255.0 * high_lvl.powf(2.4) + 0.5) as u8,
            }
            .brightness(brightness);

        write_thread_tx.send(color).unwrap();
    }
}
