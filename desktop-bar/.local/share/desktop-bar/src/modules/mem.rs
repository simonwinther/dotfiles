#[derive(Clone, Debug, Default, PartialEq)]
pub struct Memory {
    pub used: f32,
    pub total: f32,
    pub swap_used: f32,
    pub swap_total: f32,
}
pub fn parse(input: &str) -> Option<Memory> {
    let values: std::collections::HashMap<_, _> = input
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name, value.split_whitespace().next()?.parse::<u64>().ok()?))
        })
        .collect();
    let total = *values.get("MemTotal")?;
    if total == 0 {
        return None;
    }
    let available = values.get("MemAvailable").copied().unwrap_or_else(|| {
        ["MemFree", "Buffers", "Cached"]
            .iter()
            .filter_map(|k| values.get(k))
            .sum()
    });
    let swap = values.get("SwapTotal").copied().unwrap_or(0);
    let gb = |n: u64| n as f32 / 1048576.0;
    Some(Memory {
        used: gb(total.saturating_sub(available)),
        total: gb(total),
        swap_used: gb(swap.saturating_sub(values.get("SwapFree").copied().unwrap_or(0))),
        swap_total: gb(swap),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn available_memory_and_swap_are_bounded() {
        let m = parse("MemTotal: 2097152 kB\nMemAvailable: 1048576 kB\nSwapTotal: 1048576 kB\nSwapFree: 524288 kB").unwrap();
        assert_eq!(
            m,
            Memory {
                used: 1.0,
                total: 2.0,
                swap_used: 0.5,
                swap_total: 1.0
            }
        );
        assert!(parse("MemTotal: 0 kB").is_none());
    }
}
