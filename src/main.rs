#![feature(slice_patterns)]

extern crate pulse_simple;
extern crate rustfft;
extern crate num;

use std::io::prelude::*;
use std::io;
use pulse_simple::Record;
use num::Complex;

const SAMPLE_RATE: usize = 48000;
// Must be power of 2
const SAMPLES_PER_PERIOD: usize = 2048;

const BASS_CUTOFF: f32 = 450.0;
const HIGH_CUTOFF: f32 = 4600.0;

fn bin_to_freq(i: usize) -> f32 {
    (i * SAMPLE_RATE) as f32 / SAMPLES_PER_PERIOD as f32
}

const MAX_FREQ: f32 = 20_000.0;

fn pass_to(freq: f32, amp: f32, cut: f32) -> f32 {
    assert!(freq >= 0.0);

    let x = freq / cut;
    let sharpness = (cut / 400.0).powf(2.0);

    amp * f32::max(1.0 - (3.0 * sharpness).powf(7.0 * (x - 1.0)), 0.0)
}

fn band_pass(freq: f32, amp: f32, low_cut: f32, high_cut: f32) -> f32 {
    pass_from(freq, pass_to(freq, amp, high_cut), low_cut)
}

fn pass_from(freq: f32, amp: f32, cut: f32) -> f32 {
    assert!(freq >= 0.0 && cut < MAX_FREQ);

    let x = (freq - cut) / (MAX_FREQ - cut);
    let sharpness = ((MAX_FREQ - cut) / 2800.0).powf(9.6);

    amp * f32::max(1.0 - (3.0 * sharpness).powf(-7.0 * x), 0.0)
}

fn bass_pass(freq: f32, amp: f32) -> f32 {
    pass_to(freq, amp, BASS_CUTOFF)
}

fn mid_pass(freq: f32, amp: f32) -> f32 {
    band_pass(freq, amp, BASS_CUTOFF - 170.0, HIGH_CUTOFF + 800.0)
}

fn high_pass(freq: f32, amp: f32) -> f32 {
    pass_from(freq, amp, HIGH_CUTOFF)
}

/// Normalize a decibel value to [0, 1]
fn norm_db(db: f32) -> f32 {
    let x = db / 72.0;

    // Logistic function
    1.0 / (1.0 + (-10.0 * (x - 0.5)).exp())
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

fn main() {
    println!("");
    let recorder = Record::new("heliecho",
                               "Capture audio to stream as color data to adalight device",
                               None,
                               SAMPLE_RATE as u32);

    let mut stereo_data = [[0.0f32; 2]; SAMPLES_PER_PERIOD];

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
        let (max_bin_bass, max_amp_bass) = max_amp(&bass_amps_db);
        let (max_bin_mid, max_amp_mid) = max_amp(&mid_amps_db);
        let (max_bin_high, max_amp_high) = max_amp(&high_amps_db);

        print!("\rmax (f: {:6.0}, vol: {:1.5}), bass (f: {:6.0}, vol: {:1.5}), mid (f: {:6.0}, \
                vol: {:1.5}), high (f: {:6.0}, vol: {:1.5}),",
               bin_to_freq(max_bin_all),
               norm_db(max_amp_all),
               bin_to_freq(max_bin_bass),
               norm_db(max_amp_bass),
               bin_to_freq(max_bin_mid),
               norm_db(max_amp_mid),
               bin_to_freq(max_bin_high),
               norm_db(max_amp_high));
        io::stdout().flush().unwrap();
    }
}

// fn main() {
//     // Do serial writing on own thread as to not block.
//     let (write_thread_tx, write_thread_rx) = {
//         // Header to write before led data
//         let out_header = new_pixel_buf_header(leds.len() as u16);

//         // Skeleton for the output led pixel buffer to write to arduino
//         let out_pixels = repeat(RGB8 { r: 0, g: 0, b: 0 }).take(leds.len()).collect();

//         init_write_thread(&config.device.output,
//                           config.device.rate,
//                           out_header,
//                           out_pixels)
//     };

//     println!("Helion - An LED streamer\nNumber of LEDs: {}\nResize resolution: {} x {}\nCapture \
//               rate: {} fps\nLED refresh rate: {} hz\nSerial port: {}",
//              leds.len(),
//              config.framegrabber.width,
//              config.framegrabber.height,
//              config.framegrabber.frequency_Hz,
//              1.0 / led_refresh_interval,
//              config.device.output);

//     let mut led_refresh_timer = FrameTimer::new();

//     loop {
//         led_refresh_timer.tick();

//         let mut out_pixels = write_thread_rx.recv().unwrap();

//         write_thread_tx.send(out_pixels).unwrap();

//         let time_left = led_refresh_interval - led_refresh_timer.dt_to_now();
//         if time_left > 0.0 {
//             thread::sleep_ms(if time_left > 0.0 {
//                 time_left * 1_000.0
//             } else {
//                 0.0
//             } as u32);
//         }
//     }

//     println!("Hello, world!");
// }
