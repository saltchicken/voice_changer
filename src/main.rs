mod config;
mod effects;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::RingBuffer;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use config::*;
use effects::{NoiseGate, PitchShifter, RobotVoice};

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

    // Ring buffer for mono samples using the constant
    let (mut producer, mut consumer) = RingBuffer::<f32>::new((sample_rate * BUFFER_DURATION_SEC) as usize);

    // Instantiate all effects
    let mut gate = NoiseGate::new(sample_rate, GATE_THRESHOLD, GATE_ATTACK_MS, GATE_RELEASE_MS);
    let mut shifter = PitchShifter::new(sample_rate, DEFAULT_PITCH_FACTOR, WINDOW_DURATION_SEC);
    let mut robot = RobotVoice::new(sample_rate, 50.0); // 50 Hz carrier frequency
    
    // Shared atomic state: 0 = Clean, 1 = Deep, 2 = Robot
    let active_mode = Arc::new(AtomicUsize::new(1));
    let active_mode_clone = Arc::clone(&active_mode);

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
            // Read the current mode as an integer
            let mode = active_mode_clone.load(Ordering::Relaxed);
            
            for frame in data.chunks_mut(out_channels) {
                let processed = if let Ok(in_sample) = consumer.pop() {
                    
                    // 1. Gate the raw audio first to strip background noise
                    let gated_sample = gate.process(in_sample);
                    
                    // 2. Process the gated sample through the time-based effects
                    let shifted_sample = shifter.process(gated_sample);
                    let robot_sample = robot.process(gated_sample);

                    // 3. Route the audio based on the active mode
                    match mode {
                        1 => shifted_sample,
                        2 => robot_sample,
                        _ => gated_sample, // Clean audio is now gated too!
                    }
                } else {
                    0.0
                };
                
                // Apply gain and hard-limit to prevent digital clipping
                let final_sample = (processed * OUTPUT_GAIN).clamp(-1.0, 1.0);
                
                // Write the amplified sample across all output channels
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
    println!("Voice Changer is running! (Open qpwgraph to route nodes)");
    println!("Press [ENTER] to cycle through effects.");
    println!("Press Ctrl+C to stop.");
    println!("=========================================");
    
    let mode_names = ["CLEAN", "DEEP VOICE", "ROBOT VOICE"];
    let mut current_mode_index = 1; // Start on DEEP VOICE to match the atomic initial state
    println!("Current mode: {}", mode_names[current_mode_index]);

    // Terminal loop for cycling effects
    let mut input = String::new();
    loop {
        input.clear();
        io::stdin().read_line(&mut input)?; // Blocks until Enter is pressed
        
        // Cycle to the next mode and wrap around back to 0
        current_mode_index = (current_mode_index + 1) % mode_names.len();
        active_mode.store(current_mode_index, Ordering::SeqCst);
        
        println!("Current mode: {}", mode_names[current_mode_index]);
    }
}
