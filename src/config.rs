pub const WINDOW_DURATION_SEC: f32 = 0.040; // 40ms window for vocal grain overlap
pub const DEFAULT_PITCH_FACTOR: f32 = 0.75; // 0.75 = deeper, 1.25 = higher
pub const BUFFER_DURATION_SEC: u32 = 2;     // Ring buffer size in seconds
pub const OUTPUT_GAIN: f32 = 2.5;           // Volume multiplier
pub const GATE_THRESHOLD: f32 = 0.025;      // Volume required to open mic (adjust between 0.005 and 0.05)
pub const GATE_ATTACK_MS: f32 = 5.0;        // How fast the mic opens (ms)
pub const GATE_RELEASE_MS: f32 = 150.0;     // How long the mic stays open after you stop speaking (ms)
