mod config;
mod dynamics;
mod effects;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::RingBuffer;
use std::io;
use std::sync::atomic::{AtomicUsize, AtomicU32, Ordering};
use std::sync::Arc;

use config::*;
use dynamics::{NoiseGate, Compressor};
use effects::PitchShifter;

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

    let config: cpal::StreamConfig = input_device.default_input_config()?.into();
    let sample_rate = config.sample_rate.0;
    let in_channels = config.channels as usize;

    let out_config: cpal::StreamConfig = output_device.default_output_config()?.into();
    let out_channels = out_config.channels as usize;

    let (mut producer, mut consumer) = RingBuffer::<f32>::new((sample_rate * BUFFER_DURATION_SEC) as usize);

    let mut gate = NoiseGate::new(sample_rate, GATE_THRESHOLD, GATE_ATTACK_MS, GATE_RELEASE_MS);
    let mut shifter = PitchShifter::new(sample_rate, DEFAULT_PITCH_FACTOR, WINDOW_DURATION_SEC);
    let mut compressor = Compressor::new(sample_rate, -3.0, 4.0, 2.0, 100.0);
    
    // Shared atomic states
    let active_mode = Arc::new(AtomicUsize::new(1)); 
    let active_mode_clone = Arc::clone(&active_mode);

    // Track the pitch factor. (Convert f32 to u32 bits for atomic storage)
    let current_pitch = Arc::new(AtomicU32::new(DEFAULT_PITCH_FACTOR.to_bits()));
    let current_pitch_clone = Arc::clone(&current_pitch);

    let err_fn = |err| eprintln!("Audio stream error: {}", err);

    // --- CAPTURE STREAM ---
    let input_stream = input_device.build_input_stream(
        &config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
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
    let mut last_pitch_value = DEFAULT_PITCH_FACTOR;

    let output_stream = output_device.build_output_stream(
        &out_config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let mode = active_mode_clone.load(Ordering::Relaxed);
            
            // Check if the pitch has been updated from the main thread
            let new_pitch_bits = current_pitch_clone.load(Ordering::Relaxed);
            let new_pitch_value = f32::from_bits(new_pitch_bits);
            
            if (new_pitch_value - last_pitch_value).abs() > f32::EPSILON {
                shifter.set_pitch_factor(new_pitch_value);
                last_pitch_value = new_pitch_value;
            }
            
            for frame in data.chunks_mut(out_channels) {
                let processed = if let Ok(in_sample) = consumer.pop() {
                    let gated_sample = gate.process(in_sample);
                    let shifted_sample = shifter.process(gated_sample);

                    match mode {
                        1 => shifted_sample,
                        _ => gated_sample,
                    }
                } else {
                    0.0
                };
                
                let amplified = processed * OUTPUT_GAIN;
                let compressed = compressor.process(amplified);
                let final_sample = compressed.clamp(-1.0, 1.0);
                
                for sample in frame.iter_mut() {
                    *sample = final_sample;
                }
            }
        },
        err_fn,
        None,
    )?;

    input_stream.play()?;
    output_stream.play()?;

    println!("=========================================");
    println!("Voice Changer is running!");
    println!("Type a number (e.g., 0.75 = deep, 1.5 = high) and press ENTER to change pitch.");
    println!("Press [ENTER] with no text to toggle CLEAN / EFFECT mode.");
    println!("Press Ctrl+C to stop.");
    println!("=========================================");
    
    let mode_names = ["CLEAN", "EFFECT"];
    let mut current_mode_index = 1;
    println!("Current mode: {}", mode_names[current_mode_index]);

    let mut input = String::new();
    loop {
        input.clear();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();

        if trimmed.is_empty() {
            // Toggle Clean vs Effect mode
            current_mode_index = (current_mode_index + 1) % mode_names.len();
            active_mode.store(current_mode_index, Ordering::SeqCst);
            println!("Current mode: {}", mode_names[current_mode_index]);
        } else if let Ok(new_pitch) = trimmed.parse::<f32>() {
            // Update the pitch dynamically
            current_pitch.store(new_pitch.to_bits(), Ordering::SeqCst);
            println!("Pitch factor changed to: {}", new_pitch);
            
            // Automatically switch to EFFECT mode if we were bypassed
            if current_mode_index == 0 {
                current_mode_index = 1;
                active_mode.store(current_mode_index, Ordering::SeqCst);
                println!("Current mode: {}", mode_names[current_mode_index]);
            }
        } else {
            println!("Invalid input. Type a valid float (e.g. 0.8) or just hit ENTER.");
        }
    }
}
