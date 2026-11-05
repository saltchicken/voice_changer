use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::RingBuffer;
use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::io;

/// A dual-head overlapping time-domain pitch shifter (avoids clicking artifacts).
struct PitchShifter {
    buffer: Vec<f32>,
    write_idx: usize,
    phasor: f32,
    phase_inc: f32,
}

impl PitchShifter {
    fn new(sample_rate: u32, pitch_factor: f32) -> Self {
        // ~40ms window size for smooth vocal grain overlapping
        let window_size = ((sample_rate as f32) * 0.040) as usize;
        let phase_inc = (1.0 - pitch_factor) / (window_size as f32);

        Self {
            buffer: vec![0.0; window_size],
            write_idx: 0,
            phasor: 0.0,
            phase_inc,
        }
    }

    fn read_interpolated(&self, delay: f32) -> f32 {
        let len = self.buffer.len() as f32;
        let mut read_pos = (self.write_idx as f32) - delay;
        while read_pos < 0.0 {
            read_pos += len;
        }
        let r0 = (read_pos as usize) % self.buffer.len();
        let r1 = (r0 + 1) % self.buffer.len();
        let frac = read_pos.fract();

        self.buffer[r0] * (1.0 - frac) + self.buffer[r1] * frac
    }

    fn process(&mut self, input: f32) -> f32 {
        self.buffer[self.write_idx] = input;
        let len = self.buffer.len() as f32;

        // Update phasor [0.0, 1.0)
        self.phasor += self.phase_inc;
        if self.phasor >= 1.0 {
            self.phasor -= 1.0;
        } else if self.phasor < 0.0 {
            self.phasor += 1.0;
        }

        // Two read heads 180 degrees (0.5 phase) apart
        let p1 = self.phasor;
        let p2 = (self.phasor + 0.5).fract();

        let delay1 = p1 * len;
        let delay2 = p2 * len;

        // Hann window crossfade gains so head jumps are silent
        let gain1 = 0.5 * (1.0 - (2.0 * PI * p1).cos());
        let gain2 = 0.5 * (1.0 - (2.0 * PI * p2).cos());

        let out = self.read_interpolated(delay1) * gain1 + self.read_interpolated(delay2) * gain2;

        self.write_idx = (self.write_idx + 1) % self.buffer.len();
        out
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = cpal::default_host();

    let input_device = host
        .default_input_device()
        .expect("Failed to find a default input device");
    let output_device = host
        .default_output_device()
        .expect("Failed to find a default output device");

    println!("Input device:  {}", input_device.name()?);
    println!("Output device: {}", output_device.name()?);

    // Use the default configuration so input and output formats match PipeWire
    let config: cpal::StreamConfig = input_device.default_input_config()?.into();
    let sample_rate = config.sample_rate.0;
    let in_channels = config.channels as usize;

    let out_config: cpal::StreamConfig = output_device.default_output_config()?.into();
    let out_channels = out_config.channels as usize;

    // 2-second ring buffer for mono samples
    let (mut producer, mut consumer) = RingBuffer::<f32>::new((sample_rate * 2) as usize);

    // Pitch factor: 0.75 = deeper voice, 1.25 = higher voice
    let mut shifter = PitchShifter::new(sample_rate, 0.75);
    
    // Shared atomic flag for toggling the effect
    let is_clean = Arc::new(AtomicBool::new(false));
    let is_clean_clone = Arc::clone(&is_clean);

    let err_fn = |err| eprintln!("Audio stream error: {}", err);

    // --- CAPTURE STREAM ---
    let input_stream = input_device.build_input_stream(
        &config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            // Take the first channel (mono) from each frame and push to ring buffer
            for frame in data.chunks(in_channels) {
                if let Some(&sample) = frame.first() {
                    let _ = producer.push(sample);
                }
            }
        },
        err_fn,
        None,
    )?;

    // --- PLAYBACK STREAM ---
    let output_stream = output_device.build_output_stream(
        &out_config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            // Check the current toggle state (Relaxed is fast enough for audio threads)
            let bypass = is_clean_clone.load(Ordering::Relaxed);
            
            for frame in data.chunks_mut(out_channels) {
                let processed = if let Ok(in_sample) = consumer.pop() {
                    let shifted_sample = shifter.process(in_sample);
                    if bypass {
                        in_sample // Output clean audio
                    } else {
                        shifted_sample // Output pitch-shifted audio
                    }
                } else {
                    0.0
                };
                
                // Write the mono sample across all output channels
                for sample in frame.iter_mut() {
                    *sample = processed;
                }
            }
        },
        err_fn,
        None,
    )?;

    input_stream.play()?;
    output_stream.play()?;

    println!("=========================================");
    println!("Voice Changer is running! (Open qpwgraph to route nodes)");
    println!("Press [ENTER] to toggle between CLEAN and DEEP voice.");
    println!("Press Ctrl+C to stop.");
    println!("=========================================");
    println!("Current mode: DEEP");

    // Replaces the infinite sleep loop with a terminal input reader
    let mut input = String::new();
    loop {
        input.clear();
        io::stdin().read_line(&mut input)?; // Blocks until Enter is pressed
        
        // Toggle the atomic flag
        let current_state = is_clean.load(Ordering::SeqCst);
        let new_state = !current_state;
        is_clean.store(new_state, Ordering::SeqCst);
        
        if new_state {
            println!("Current mode: CLEAN (Bypassed)");
        } else {
            println!("Current mode: DEEP (Shifted)");
        }
    }
}
