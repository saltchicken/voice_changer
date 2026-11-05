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
