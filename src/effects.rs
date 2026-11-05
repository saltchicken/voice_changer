use std::f32::consts::PI;

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

    fn read_hermite(&self, delay: f32) -> f32 {
        let len = self.buffer.len() as f32;
        let mut read_pos = (self.write_idx as f32) - delay;
        
        // Handle buffer wrap-around
        while read_pos < 0.0 {
            read_pos += len;
        }

        // Get the 4 surrounding sample indices for the cubic curve
        let r1 = (read_pos as usize) % self.buffer.len();
        let r0 = if r1 == 0 { self.buffer.len() - 1 } else { r1 - 1 };
        let r2 = (r1 + 1) % self.buffer.len();
        let r3 = (r2 + 1) % self.buffer.len();

        let frac = read_pos.fract();

        let y0 = self.buffer[r0];
        let y1 = self.buffer[r1];
        let y2 = self.buffer[r2];
        let y3 = self.buffer[r3];

        // 4-point, 3rd-order Hermite polynomial calculation
        let c0 = y1;
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 1.5 * (y1 - y2) + 0.5 * (y3 - y0);

        c0 + frac * (c1 + frac * (c2 + frac * c3))
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

        let out = self.read_hermite(delay1) * gain1 + self.read_hermite(delay2) * gain2;

        self.write_idx = (self.write_idx + 1) % self.buffer.len();
        out
    }
}
