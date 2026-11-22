//! Decides which windows get a label and where it sits.
use crate::hyprland::{Client, Monitor};
use std::collections::HashSet;

/// Logical height of a label.
pub const LABEL_HEIGHT: i32 = 16;
/// How far a label reaches below the window's top edge. The rest sits in the
/// gap above the window, so it hides as little content as possible.
const OVERLAP: i32 = 3;
/// Distance from the window's left edge to the label.
const INSET: i32 = 14;
/// Narrower windows are not labelled.
const MIN_WIDTH: i32 = 48;

#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub address: String,
    pub output: String,
    pub scale: f64,
    /// Output-local logical position of the label's top-left corner.
    pub x: i32,
    pub y: i32,
    pub max_width: i32,
    pub title: String,
    pub active: bool,
}

pub fn placements(clients: &[Client], monitors: &[Monitor]) -> Vec<Placement> {
    let fullscreen_workspaces: HashSet<i64> = clients
        .iter()
        .filter(|c| c.mapped && c.fullscreen > 0)
        .map(|c| c.workspace.id)
        .collect();
    // An open special workspace covers the monitor's regular workspace.
    let shown_workspaces: HashSet<i64> = monitors
        .iter()
        .map(|m| match m.special_workspace.id {
            0 => m.active_workspace.id,
            special => special,
        })
        .collect();

    clients
        .iter()
        .filter(|c| c.mapped && !c.hidden && c.visible)
        .filter(|c| shown_workspaces.contains(&c.workspace.id))
        // Grouped windows already show their titles in the groupbar.
        .filter(|c| c.grouped.is_empty())
        // Fullscreen covers everything else on the workspace and leaves no
        // border to label; a maximized window keeps its label.
        .filter(|c| c.fullscreen != 2)
        .filter(|c| c.fullscreen > 0 || !fullscreen_workspaces.contains(&c.workspace.id))
        .filter(|c| !c.title.trim().is_empty())
        .filter_map(|c| {
            let monitor = monitors.iter().find(|m| m.id == c.monitor)?;
            let max_width = c.size[0] - 2 * INSET;
            (max_width >= MIN_WIDTH).then(|| Placement {
                address: c.address.clone(),
                output: monitor.name.clone(),
                scale: monitor.scale,
                x: (c.at[0] - monitor.x + INSET).max(0),
                y: (c.at[1] - monitor.y + OVERLAP - LABEL_HEIGHT).max(0),
                max_width,
                title: c.title.trim().to_owned(),
                active: c.focus_history_id == 0,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hyprland::WorkspaceRef;

    fn monitor() -> Monitor {
        Monitor {
            id: 0,
            name: "eDP-1".into(),
            x: 160,
            y: 1080,
            scale: 1.2,
            active_workspace: WorkspaceRef { id: 1 },
            special_workspace: WorkspaceRef { id: 0 },
        }
    }

    fn client(address: &str, title: &str) -> Client {
        Client {
            address: address.into(),
            mapped: true,
            hidden: false,
            visible: true,
            at: [172, 1092],
            size: [800, 400],
            workspace: WorkspaceRef { id: 1 },
            monitor: 0,
            title: title.into(),
            fullscreen: 0,
            grouped: vec![],
            focus_history_id: 1,
        }
    }

    #[test]
    fn places_label_on_top_border_in_output_coordinates() {
        let mut lower = client("0x1", " Claude Code ");
        lower.at[1] = 1080 + 500;
        let labels = placements(&[lower], &[monitor()]);
        assert_eq!(labels.len(), 1);
        let label = &labels[0];
        assert_eq!((label.x, label.y), (12 + INSET, 500 + OVERLAP - LABEL_HEIGHT));
        assert_eq!(label.title, "Claude Code");
        assert_eq!(label.output, "eDP-1");
        assert!(!label.active);
    }

    #[test]
    fn clamps_label_at_top_of_output() {
        let labels = placements(&[client("0x1", "top")], &[monitor()]);
        assert_eq!(labels[0].y, 0);
    }

    #[test]
    fn skips_grouped_hidden_untitled_and_offscreen_windows() {
        let mut grouped = client("0x1", "a");
        grouped.grouped = vec!["0x1".into()];
        let mut hidden = client("0x2", "b");
        hidden.hidden = true;
        let untitled = client("0x3", "  ");
        let mut elsewhere = client("0x4", "c");
        elsewhere.workspace.id = 7;
        let shown = client("0x5", "d");
        let labels = placements(&[grouped, hidden, untitled, elsewhere, shown], &[monitor()]);
        let addresses: Vec<_> = labels.iter().map(|l| l.address.as_str()).collect();
        assert_eq!(addresses, ["0x5"]);
    }

    #[test]
    fn fullscreen_hides_labels_on_its_workspace_but_maximized_keeps_one() {
        let mut full = client("0x1", "video");
        full.fullscreen = 2;
        let behind = client("0x2", "terminal");
        assert!(placements(&[full, behind.clone()], &[monitor()]).is_empty());

        let mut maximized = client("0x3", "editor");
        maximized.fullscreen = 1;
        let labels = placements(&[maximized, behind], &[monitor()]);
        let addresses: Vec<_> = labels.iter().map(|l| l.address.as_str()).collect();
        assert_eq!(addresses, ["0x3"]);
    }

    #[test]
    fn open_special_workspace_hides_labels_behind_it() {
        let mut with_special = monitor();
        with_special.special_workspace.id = -97;
        let behind = client("0x1", "regular");
        let mut scratch = client("0x2", "scratch");
        scratch.workspace.id = -97;
        let labels = placements(&[behind, scratch], &[with_special]);
        let addresses: Vec<_> = labels.iter().map(|l| l.address.as_str()).collect();
        assert_eq!(addresses, ["0x2"]);
    }

    #[test]
    fn marks_focused_window_active() {
        let mut focused = client("0x1", "a");
        focused.focus_history_id = 0;
        assert!(placements(&[focused], &[monitor()])[0].active);
    }
}
