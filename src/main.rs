use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::RingBuffer;
use std::f32::consts::PI;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// --- CONFIGURATION CONSTANTS ---
const WINDOW_DURATION_SEC: f32 = 0.040; // 40ms window for vocal grain overlap
const DEFAULT_PITCH_FACTOR: f32 = 0.75; // 0.75 = deeper, 1.25 = higher
const BUFFER_DURATION_SEC: u32 = 2;     // Ring buffer size in seconds
const OUTPUT_GAIN: f32 = 2.5;           // Volume multiplier
// -------------------------------

/// A dual-head overlapping time-domain pitch shifter (avoids clicking artifacts).
struct PitchShifter {
    buffer: Vec<f32>,
    write_idx: usize,
    phasor: f32,
    phase_inc: f32,
}

impl PitchShifter {
    fn new(sample_rate: u32, pitch_factor: f32) -> Self {
        let window_size = ((sample_rate as f32) * WINDOW_DURATION_SEC) as usize;
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

/// A simple ring modulation effect for a robotic "Dalek" voice.
struct RobotVoice {
    phasor: f32,
    phase_inc: f32,
}

impl RobotVoice {
    fn new(sample_rate: u32, frequency: f32) -> Self {
        Self {
            phasor: 0.0,
            phase_inc: frequency / sample_rate as f32,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        self.phasor += self.phase_inc;
        if self.phasor >= 1.0 {
            self.phasor -= 1.0;
        }
        // Multiply input by a sine wave to create a robotic ring modulation
        let carrier = (self.phasor * 2.0 * PI).sin();
        input * carrier
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

    // Ring buffer for mono samples using the constant
    let (mut producer, mut consumer) = RingBuffer::<f32>::new((sample_rate * BUFFER_DURATION_SEC) as usize);

    // Instantiate both effects
    let mut shifter = PitchShifter::new(sample_rate, DEFAULT_PITCH_FACTOR);
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
                    
                    // Always process time-based effects to keep their buffers updated
                    let shifted_sample = shifter.process(in_sample);
                    let robot_sample = robot.process(in_sample);

                    // Route the audio based on the active mode
                    match mode {
                        1 => shifted_sample,
                        2 => robot_sample,
                        _ => in_sample, // Mode 0 (or fallback) is clean audio
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
