use super::{Identity, Monitor};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    Left,
    Right,
    #[default]
    Below,
}
impl Position {
    pub const ALL: [Self; 3] = [Self::Left, Self::Right, Self::Below];
    pub fn next(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Below,
            Self::Below => Self::Left,
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedProfile {
    pub name: String,
    pub monitors: Vec<FixedMonitor>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedMonitor {
    pub identity: Identity,
    pub mode: String,
    pub scale: f64,
    pub transform: u8,
    pub x: i32,
    pub y: i32,
}
impl FixedProfile {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.is_empty() && !self.monitors.is_empty() && self.monitors.len() <= 8,
            "empty fixed profile"
        );
        for m in &self.monitors {
            ensure!(
                !m.identity.make.is_empty() && !m.identity.model.is_empty(),
                "profile {} requires make and model",
                self.name
            );
            logical_size(&m.mode, m.scale, m.transform)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Rule {
    pub output: String,
    pub mode: String,
    pub scale: f64,
    pub transform: u8,
    pub x: i32,
    pub y: i32,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DisplayState {
    pub monitors: Vec<Monitor>,
    pub profile: String,
    pub setup: Option<String>,
    pub position: Option<Position>,
    pub switchable: bool,
    pub diagnostic: Option<String>,
}

pub fn mode_parts(mode: &str) -> Result<(u32, u32, f64)> {
    let clean = mode.trim_end_matches("Hz");
    let (size, hz) = clean
        .split_once('@')
        .ok_or_else(|| anyhow::anyhow!("mode requires refresh rate: {mode}"))?;
    let (w, h) = size
        .split_once('x')
        .ok_or_else(|| anyhow::anyhow!("invalid mode: {mode}"))?;
    let (w, h, hz): (u32, u32, f64) = (w.parse()?, h.parse()?, hz.parse()?);
    ensure!(
        w > 0 && h > 0 && hz.is_finite() && hz > 0.0,
        "invalid mode {mode}"
    );
    Ok((w, h, hz))
}
pub fn logical_size(mode: &str, scale: f64, transform: u8) -> Result<(i32, i32)> {
    ensure!(
        scale.is_finite() && (0.5..=4.0).contains(&scale) && transform <= 7,
        "invalid scale or transform"
    );
    let (w, h, _) = mode_parts(mode)?;
    let (w, h) = if transform % 2 == 1 { (h, w) } else { (w, h) };
    // Hyprland requires integral logical dimensions for a valid fractional scale.
    let (w, h) = (w as f64 / scale, h as f64 / scale);
    ensure!(
        (w - w.round()).abs() < 0.02 && (h - h.round()).abs() < 0.02,
        "scale does not produce integral logical dimensions"
    );
    Ok((w.round() as i32, h.round() as i32))
}
pub fn supported(m: &Monitor, wanted: &str) -> bool {
    let Ok((w, h, hz)) = mode_parts(wanted) else {
        return false;
    };
    m.modes.iter().any(|a| {
        mode_parts(a).is_ok_and(|(aw, ah, ar)| w == aw && h == ah && (hz - ar).abs() < 0.1)
    })
}
fn current_mode(m: &Monitor) -> String {
    let exact = format!("{}x{}@{:.2}", m.width, m.height, m.refresh);
    if supported(m, &exact) {
        exact
    } else {
        m.modes
            .first()
            .cloned()
            .unwrap_or(exact)
            .trim_end_matches("Hz")
            .into()
    }
}
pub fn current_rule(m: &Monitor) -> Rule {
    Rule {
        output: m.name.clone(),
        mode: current_mode(m),
        scale: if m.scale > 0.0 { m.scale } else { 1.0 },
        transform: m.transform,
        x: m.x,
        y: m.y,
    }
}

// Reject every ambiguous role assignment, including two physically indistinguishable units.
pub fn match_profile(profile: &FixedProfile, monitors: &[Monitor]) -> Option<Vec<usize>> {
    if profile.monitors.len() != monitors.len() {
        return None;
    }
    fn search(
        p: &FixedProfile,
        ms: &[Monitor],
        role: usize,
        used: &mut Vec<usize>,
        matches: &mut Vec<(u32, Vec<usize>)>,
        score: u32,
    ) {
        if role == p.monitors.len() {
            matches.push((score, used.clone()));
            return;
        }
        for (i, m) in ms.iter().enumerate() {
            if !used.contains(&i)
                && let Some(s) = p.monitors[role].identity.score(m)
            {
                used.push(i);
                search(p, ms, role + 1, used, matches, score + s);
                used.pop();
            }
        }
    }
    let mut found = vec![];
    search(profile, monitors, 0, &mut vec![], &mut found, 0);
    found.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    if found.len() > 1 && found[0].0 == found[1].0 {
        return None;
    }
    found.first().map(|(_, v)| v.clone())
}
pub fn fixed_plan(
    profiles: &[FixedProfile],
    ms: &[Monitor],
) -> Result<Option<(String, Vec<Rule>)>> {
    let hits: Vec<_> = profiles
        .iter()
        .filter_map(|p| match_profile(p, ms).map(|ids| (p, ids)))
        .collect();
    let mut hits = hits;
    hits.sort_by_key(|(profile, ids)| {
        std::cmp::Reverse(
            profile
                .monitors
                .iter()
                .zip(ids)
                .map(|(r, i)| r.identity.score(&ms[*i]).unwrap_or(0))
                .sum::<u32>(),
        )
    });
    if hits.len() > 1 {
        let score = |(p, ids): &(&FixedProfile, Vec<usize>)| {
            p.monitors
                .iter()
                .zip(ids)
                .map(|(r, i)| r.identity.score(&ms[*i]).unwrap_or(0))
                .sum::<u32>()
        };
        ensure!(
            score(&hits[0]) > score(&hits[1]),
            "multiple equally specific fixed profiles match; preserving the current layout"
        );
    }
    let Some((p, ids)) = hits.first() else {
        return Ok(None);
    };
    let mut rules = vec![];
    for (wanted, id) in p.monitors.iter().zip(ids) {
        let m = &ms[*id];
        ensure!(
            supported(m, &wanted.mode),
            "{} does not advertise {}; preserving current layout",
            m.description,
            wanted.mode
        );
        logical_size(&wanted.mode, wanted.scale, wanted.transform)?;
        rules.push(Rule {
            output: m.name.clone(),
            mode: wanted.mode.clone(),
            scale: wanted.scale,
            transform: wanted.transform,
            x: wanted.x,
            y: wanted.y,
        });
    }
    Ok(Some((p.name.clone(), rules)))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorPreference {
    pub identity: Identity,
    pub mode: String,
    pub scale: f64,
    pub transform: u8,
}
pub fn laptop_plan(ms: &[Monitor], position: Position, scale: f64) -> Result<Vec<Rule>> {
    configured_laptop_plan(ms, position, scale, &[])
}
pub fn configured_laptop_plan(
    ms: &[Monitor],
    position: Position,
    scale: f64,
    preferences: &[MonitorPreference],
) -> Result<Vec<Rule>> {
    let internal: Vec<_> = ms.iter().filter(|m| m.internal()).collect();
    let external: Vec<_> = ms.iter().filter(|m| !m.internal()).collect();
    ensure!(
        internal.len() == 1 && external.len() == 1,
        "laptop arrangements require one panel and one external screen"
    );
    let mut i = current_rule(internal[0]);
    let mut e = current_rule(external[0]);
    i.scale = scale;
    for (rule, monitor) in [(&mut i, internal[0]), (&mut e, external[0])] {
        let mut matches: Vec<_> = preferences
            .iter()
            .filter_map(|p| p.identity.score(monitor).map(|s| (s, p)))
            .collect();
        matches.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
        ensure!(
            matches.len() < 2 || matches[0].0 > matches[1].0,
            "ambiguous laptop monitor preferences"
        );
        if let Some((_, p)) = matches.first() {
            ensure!(
                supported(monitor, &p.mode),
                "{} does not advertise preferred mode {}",
                monitor.model,
                p.mode
            );
            rule.mode = p.mode.clone();
            rule.scale = p.scale;
            rule.transform = p.transform;
        }
    }

    let (iw, ih) = logical_size(&i.mode, i.scale, i.transform)?;
    let (ew, eh) = logical_size(&e.mode, e.scale, e.transform)?;
    match position {
        Position::Left => {
            i.x = 0;
            i.y = ((eh - ih) / 2).max(0);
            e.x = iw;
            e.y = ((ih - eh) / 2).max(0);
        }
        Position::Right => {
            e.x = 0;
            e.y = ((ih - eh) / 2).max(0);
            i.x = ew;
            i.y = ((eh - ih) / 2).max(0);
        }
        Position::Below => {
            e.x = ((iw - ew) / 2).max(0);
            e.y = 0;
            i.x = ((ew - iw) / 2).max(0);
            i.y = eh;
        }
    }
    Ok(vec![e, i])
}
pub fn fallback_plan(ms: &[Monitor]) -> Vec<Rule> {
    // Existing working displays stay exactly as they are; new/disabled outputs are enabled to the right.
    let mut right = ms
        .iter()
        .filter(|m| !m.disabled)
        .map(|m| {
            let r = current_rule(m);
            m.x + logical_size(&r.mode, r.scale, r.transform).map_or(m.width as i32, |s| s.0)
        })
        .max()
        .unwrap_or(0);
    ms.iter()
        .filter(|m| m.disabled)
        .map(|m| {
            let mut r = current_rule(m);
            r.scale = 1.0;
            r.transform = 0;
            r.x = right;
            r.y = 0;
            right += logical_size(&r.mode, 1.0, 0).map_or(m.width as i32, |s| s.0);
            r
        })
        .collect()
}
pub fn needs_apply(rule: &Rule, m: &Monitor) -> bool {
    let same_mode = mode_parts(&rule.mode)
        .is_ok_and(|(w, h, hz)| m.width == w && m.height == h && (m.refresh - hz).abs() < 0.15);
    m.disabled
        || !same_mode
        || (m.scale - rule.scale).abs() > 0.001
        || m.transform != rule.transform
        || m.x != rule.x
        || m.y != rule.y
}
pub fn validate_plan(rules: &[Rule], ms: &[Monitor]) -> Result<()> {
    if ms.is_empty() {
        bail!("no connected displays; no changes made");
    }
    let mut names = std::collections::HashSet::new();
    for r in rules {
        ensure!(names.insert(&r.output), "duplicate output in plan");
        let m = ms
            .iter()
            .find(|m| m.name == r.output)
            .ok_or_else(|| anyhow::anyhow!("output disconnected"))?;
        ensure!(
            supported(m, &r.mode),
            "unsupported mode {} for {}",
            r.mode,
            r.output
        );
        logical_size(&r.mode, r.scale, r.transform)?;
    }
    Ok(())
}
