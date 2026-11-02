#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Times {
    idle: u64,
    total: u64,
}

pub fn parse(input: &str) -> Vec<Times> {
    input
        .lines()
        .take_while(|line| line.starts_with("cpu"))
        .filter_map(|line| {
            let numbers: Vec<u64> = line
                .split_whitespace()
                .skip(1)
                .take(8)
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            if numbers.len() < 4 {
                return None;
            }
            Some(Times {
                idle: numbers[3] + numbers.get(4).copied().unwrap_or(0),
                total: numbers.iter().sum(),
            })
        })
        .collect()
}
pub fn usage(previous: &[Times], current: &[Times]) -> Vec<f32> {
    current
        .iter()
        .zip(previous)
        .map(|(c, p)| {
            let total = c.total.saturating_sub(p.total);
            let idle = c.idle.saturating_sub(p.idle).min(total);
            if total == 0 {
                0.0
            } else {
                100.0 * (total - idle) as f32 / total as f32
            }
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_iowait_as_idle_and_excludes_guest_double_counting() {
        let a = parse("cpu 100 0 100 800 0 0 0 0 200 0\ncpu0 50 0 50 400\nintr 0");
        let b = parse("cpu 150 0 100 850 0 0 0 0 250 0\ncpu0 75 0 50 425\nintr 0");
        assert_eq!(usage(&a, &b), vec![50.0, 50.0]);
        assert!(parse("cpu bad").is_empty());
        assert_eq!(usage(&b, &a), vec![0.0, 0.0]);
    }
}
