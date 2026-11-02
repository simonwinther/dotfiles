use std::path::Path;
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Battery {
    pub percent: f32,
    pub charging: bool,
    pub plugged: bool,
    pub watts: f32,
    pub hours: f32,
}
pub fn parse(input: &str) -> Option<Battery> {
    let values: std::collections::HashMap<_, _> = input
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    let number = |key| {
        values
            .get(key)
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|n| n.is_finite())
    };
    let percent = number("POWER_SUPPLY_CAPACITY")?.clamp(0.0, 100.0);
    let charging = values.get("POWER_SUPPLY_STATUS") == Some(&"Charging");
    let plugged = charging
        || values
            .get("POWER_SUPPLY_STATUS")
            .is_some_and(|s| ["Full", "Not charging"].contains(s));
    let watts = number("POWER_SUPPLY_POWER_NOW")
        .map(|p| p / 1e6)
        .unwrap_or_else(|| {
            number("POWER_SUPPLY_CURRENT_NOW").unwrap_or(0.0)
                * number("POWER_SUPPLY_VOLTAGE_NOW").unwrap_or(0.0)
                / 1e12
        })
        .abs();
    let energy = number("POWER_SUPPLY_ENERGY_NOW").unwrap_or_else(|| {
        number("POWER_SUPPLY_CHARGE_NOW").unwrap_or(0.0)
            * number("POWER_SUPPLY_VOLTAGE_NOW").unwrap_or(0.0)
            / 1e6
    }) / 1e6;
    let full = number("POWER_SUPPLY_ENERGY_FULL").unwrap_or_else(|| {
        number("POWER_SUPPLY_CHARGE_FULL").unwrap_or(0.0)
            * number("POWER_SUPPLY_VOLTAGE_NOW").unwrap_or(0.0)
            / 1e6
    }) / 1e6;
    Some(Battery {
        percent,
        charging,
        plugged,
        watts,
        hours: if watts > 0.01 {
            if charging {
                (full - energy).max(0.0) / watts
            } else {
                energy / watts
            }
        } else {
            0.0
        },
    })
}
pub fn read() -> Option<Battery> {
    let mut batteries: Vec<_> = std::fs::read_dir("/sys/class/power_supply")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    batteries.sort();
    batteries
        .iter()
        .filter(|p| std::fs::read_to_string(p.join("type")).is_ok_and(|s| s.trim() == "Battery"))
        .find_map(|p| read_at(p))
}
fn read_at(path: &Path) -> Option<Battery> {
    parse(&std::fs::read_to_string(path.join("uevent")).ok()?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn charge_units_and_time_to_full_are_converted() {
        let b = parse("POWER_SUPPLY_CAPACITY=40\nPOWER_SUPPLY_STATUS=Charging\nPOWER_SUPPLY_CURRENT_NOW=1000000\nPOWER_SUPPLY_VOLTAGE_NOW=12000000\nPOWER_SUPPLY_CHARGE_NOW=2000000\nPOWER_SUPPLY_CHARGE_FULL=5000000").unwrap();
        assert_eq!(b.watts, 12.0);
        assert_eq!(b.hours, 3.0);
        assert!(b.charging);
        assert!(parse("POWER_SUPPLY_CAPACITY=NaN").is_none());
    }
}
