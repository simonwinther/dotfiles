//! Use the terminal's background and configured ANSI palette on light and dark themes.
use ratatui::style::{Color, Modifier, Style};

pub const BG: Color = Color::Reset;
pub const PANEL: Color = Color::Reset;
pub const FG: Color = Color::Reset;
pub const DIM: Color = Color::DarkGray;
pub const EDGE: Color = Color::DarkGray;
pub const ACCENT: Color = Color::Yellow;
pub const GREEN: Color = Color::Green;
pub const BLUE: Color = Color::Blue;
pub const RED: Color = Color::Red;

pub fn selection_style() -> Style {
    Style::default()
        .fg(FG)
        .bg(BG)
        .add_modifier(Modifier::BOLD | Modifier::REVERSED)
}

pub fn focus_style() -> Style {
    Style::default()
        .fg(FG)
        .bg(BG)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}
