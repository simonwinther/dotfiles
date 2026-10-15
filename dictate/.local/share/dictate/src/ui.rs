use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct View {
    pub phase: String,
    pub transcript: String,
    pub bars: Vec<f32>,
    pub start_ms: u64,
    pub recording_ms: u64,
    pub live: bool,
    pub cost_rate: f64,
    pub currency: String,
    pub exchange: f64,
    pub monitor: String,
}
impl Default for View {
    fn default() -> Self {
        Self {
            phase: "starting".into(),
            transcript: String::new(),
            bars: vec![0.; 42],
            start_ms: 0,
            recording_ms: 0,
            live: true,
            cost_rate: 0.,
            currency: "DKK".into(),
            exchange: 6.4104,
            monitor: String::new(),
        }
    }
}
