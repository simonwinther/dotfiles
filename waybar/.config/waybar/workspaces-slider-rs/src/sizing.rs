//! UI sizing is separate from the compositor's output buffer scale.
use crate::render::{FIRST_CENTER, SLOT_WIDTH, WORKSPACE_COUNT};

pub fn configured_scale() -> f32 {
    let config = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")));
    config.and_then(|p| std::fs::read(p.join("desktop/bar-outputs.json")).ok())
        .and_then(|data| serde_json::from_slice::<serde_json::Value>(&data).ok())
        .and_then(|v| v["scale"].as_f64())
        .filter(|s| s.is_finite() && (0.75..=2.0).contains(s))
        .unwrap_or(1.0) as f32
}

pub fn workspace_at(x: f64, ui_scale: f32) -> i32 {
    let workspace = ((x as f32 / ui_scale - FIRST_CENTER) / SLOT_WIDTH).round() as i32 + 1;
    workspace.clamp(1, WORKSPACE_COUNT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{center_for, Renderer, PANEL_WIDTH, PANEL_HEIGHT};

    #[test]
    fn scaled_clicks_select_the_visually_corresponding_workspace() {
        for scale in [0.75, 1.0, 1.2, 1.5, 2.0] {
            for workspace in 1..=WORKSPACE_COUNT {
                assert_eq!(workspace_at((center_for(workspace) * scale) as f64, scale), workspace);
            }
            assert_eq!(workspace_at(-100.0, scale), 1);
            assert_eq!(workspace_at(9999.0, scale), WORKSPACE_COUNT);
        }
    }

    #[test]
    fn fractional_ui_size_keeps_wayland_buffer_dimensions_divisible() {
        for scale in [1.0, 1.2, 1.35] {
            for device in [1, 2, 3] {
                let renderer = Renderer::for_surface(scale, device).unwrap();
                assert_eq!(renderer.width(), (PANEL_WIDTH * scale).round() as u32 * device as u32);
                assert_eq!(renderer.height(), (PANEL_HEIGHT * scale).round() as u32 * device as u32);
                assert_eq!(renderer.width() % device as u32, 0);
                assert_eq!(renderer.height() % device as u32, 0);
            }
        }
    }
}
