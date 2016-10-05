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

// For light to pulsate to the beat, calculate using freqs of max MAX_BREAT_FREQ
const MAX_BEAT_FREQ: f64 = 400.0;

// Frequency at first index gets weight `1 + LOW_FREQ_WEIGHT`,
// while MAX_BEAT_FREQ gets weight `1 - LOW_FREQ_WEIGHT`
const LOW_FREQ_WEIGHT: f64 = 0.3;

fn fft_i_to_freq(i: usize) -> f64 {
    (i * SAMPLE_RATE) as f64 / SAMPLES_PER_PERIOD as f64
}

fn main() {
    println!("");
    let recorder = Record::new("heliecho",
                               "Capture audio to stream as color data to adalight device",
                               None,
                               SAMPLE_RATE as u32);

    let mut stereo_data = [[0.0f32; 2]; SAMPLES_PER_PERIOD]; // 10ms of audio data for 2 channels

    loop {
        // Record
        recorder.read(&mut stereo_data);

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

        let mut freq_amps_complex = [complex_zero; SAMPLES_PER_PERIOD];

        fft.process(&avg_data as &[_], &mut freq_amps_complex as &mut [_]);

        let mut freq_amps = [0.0f32; SAMPLES_PER_PERIOD >> 1];

        for (i, &c) in freq_amps_complex.iter().take(SAMPLES_PER_PERIOD / 2).enumerate() {
            freq_amps[i] = (c.re.powi(2) + c.im.powi(2)).sqrt();
        }

        let (mut max_i, mut max_e) = (0, 0.0);

        for i in 0..freq_amps.len() {
            if freq_amps[i] > max_e {
                max_e = freq_amps[i];
                max_i = i;
            }
        }

        print!("\rloud freq: {:7.0} max amp: {:8.3}",
               fft_i_to_freq(max_i),
               20.0 * max_e.log(10.0));
        io::stdout().flush();
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
