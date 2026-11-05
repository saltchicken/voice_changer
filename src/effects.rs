use std::f32::consts::PI;

/// A standard Noise Gate to eliminate background noise when not speaking.
pub struct NoiseGate {
    threshold: f32,
    envelope: f32,
    gain: f32,
    attack_coef: f32,
    release_coef: f32,
}

impl NoiseGate {
    pub fn new(sample_rate: u32, threshold: f32, attack_ms: f32, release_ms: f32) -> Self {
        Self {
            threshold,
            envelope: 0.0,
            gain: 0.0,
            // Calculate standard DSP smoothing coefficients based on sample rate
            attack_coef: (-1.0 / ((attack_ms / 1000.0) * sample_rate as f32)).exp(),
            release_coef: (-1.0 / ((release_ms / 1000.0) * sample_rate as f32)).exp(),
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let abs_in = input.abs();
        
        // Track the volume envelope of the incoming audio
        let env_coef = if abs_in > self.envelope { self.attack_coef } else { self.release_coef };
        self.envelope = env_coef * self.envelope + (1.0 - env_coef) * abs_in;

        // Target gain is 1.0 (open) if we are above threshold, or 0.0 (muted) if below
        let target_gain = if self.envelope > self.threshold { 1.0 } else { 0.0 };
        
        // Smoothly transition the actual volume so the gate doesn't cause clicking artifacts
        let gain_coef = if target_gain > self.gain { self.attack_coef } else { self.release_coef };
        self.gain = gain_coef * self.gain + (1.0 - gain_coef) * target_gain;

        input * self.gain
    }
}

/// A dual-head overlapping time-domain pitch shifter (avoids clicking artifacts).
pub struct PitchShifter {
    buffer: Vec<f32>,
    write_idx: usize,
    phasor: f32,
    phase_inc: f32,
}

impl PitchShifter {
    pub fn new(sample_rate: u32, pitch_factor: f32, window_duration_sec: f32) -> Self {
        let window_size = ((sample_rate as f32) * window_duration_sec) as usize;
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

    pub fn process(&mut self, input: f32) -> f32 {
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
pub struct RobotVoice {
    phasor: f32,
    phase_inc: f32,
}

impl RobotVoice {
    pub fn new(sample_rate: u32, frequency: f32) -> Self {
        Self {
            phasor: 0.0,
            phase_inc: frequency / sample_rate as f32,
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        self.phasor += self.phase_inc;
        if self.phasor >= 1.0 {
            self.phasor -= 1.0;
        }
        // Multiply input by a sine wave to create a robotic ring modulation
        let carrier = (self.phasor * 2.0 * PI).sin();
        input * carrier
    }
}
