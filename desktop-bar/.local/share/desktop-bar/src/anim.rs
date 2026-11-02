#[derive(Clone, Debug)]
pub struct Spring {
    pub value: f32,
    pub target: f32,
    velocity: f32,
    frequency: f32,
    damping: f32,
}

impl Spring {
    pub fn new(value: f32, frequency: f32, damping: f32) -> Self {
        Self {
            value,
            target: value,
            velocity: 0.0,
            frequency,
            damping,
        }
    }

    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }
    pub fn tune(&mut self, frequency: f32, damping: f32) {
        self.frequency = frequency;
        self.damping = damping;
    }

    pub fn moving(&self) -> bool {
        (self.value - self.target).abs() > 0.02 || self.velocity.abs() > 0.05
    }

    pub fn step(&mut self, dt: f32) -> bool {
        // Solve the damped oscillator exactly so 60/120/144 Hz and irregular
        // frame delivery follow the same curve, including mid-flight reversals.
        let dt = dt.clamp(0.0, 0.25);
        let y = self.value - self.target;
        let v = self.velocity;
        let w = self.frequency;
        let z = self.damping;
        let (position, velocity) = if z < 0.999 {
            let d = w * (1.0 - z * z).sqrt();
            let decay = (-z * w * dt).exp();
            let (sin, cos) = (d * dt).sin_cos();
            (
                decay * (y * cos + (v + z * w * y) / d * sin),
                decay * (v * cos - (z * w * v + w * w * y) / d * sin),
            )
        } else if z <= 1.001 {
            let decay = (-w * dt).exp();
            let c = v + w * y;
            (decay * (y + c * dt), decay * (v - w * c * dt))
        } else {
            let d = (z * z - 1.0).sqrt();
            let a = -w * (z - d);
            let b = -w * (z + d);
            let first = (v - b * y) / (a - b) * (a * dt).exp();
            let second = (a * y - v) / (a - b) * (b * dt).exp();
            (first + second, a * first + b * second)
        };
        self.value = self.target + position;
        self.velocity = velocity;
        if !self.moving() {
            self.snap(self.target);
        }
        self.moving()
    }
}

pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn springs_converge_after_stalls_and_retargeting() {
        for damping in [0.65, 1.0] {
            let mut s = Spring::new(0.0, 24.0, damping);
            s.target = 250.0;
            for _ in 0..180 {
                s.step(1.0 / 60.0);
            }
            assert!(!s.moving());
            assert_eq!(s.value, 250.0);
            s.target = -75.0;
            for _ in 0..180 {
                s.step(1.0);
            }
            assert_eq!(s.value, -75.0);
        }
    }
    #[test]
    fn spring_motion_matches_across_refresh_rates_and_bounces() {
        let sample = |hz: u32| {
            let mut spring = Spring::new(36.0, 21.0, 0.62);
            spring.target = 300.0;
            let mut peak = 0.0_f32;
            for _ in 0..hz / 2 {
                spring.step(1.0 / hz as f32);
                peak = peak.max(spring.value);
            }
            (spring, peak)
        };
        let (sixty, peak) = sample(60);
        let (fast, _) = sample(144);
        assert!((sixty.value - fast.value).abs() < 0.001);
        assert!((sixty.velocity - fast.velocity).abs() < 0.01);
        assert!((310.0..330.0).contains(&peak));
        let mut reversed = sixty;
        reversed.target = 36.0;
        let previous = reversed.value;
        reversed.step(1.0 / 144.0);
        assert!((reversed.value - previous).abs() < 5.0);
    }
}
