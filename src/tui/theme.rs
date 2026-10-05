//! TUI theme — wraps [`crate::theme::Theme`] with the colors ratatui needs.

use ratatui::style::{Color, Modifier, Style};

use crate::theme::{theme_for, Theme as CoreTheme, ThemeMode};

pub use crate::theme::{next_theme_mode, THEME_MODES};

/// A TUI-friendly theme — pre-computes ratatui `Color` values for fast lookup
/// in the hot render path.
#[derive(Debug, Clone)]
pub struct TuiTheme {
    pub mode: String,
    pub primary: Color,
    pub gray: Color,
    pub background: Color,
    pub dim_secondary: bool,
    pub inverse_button: bool,
}

impl TuiTheme {
    /// `auto` mode — leave colors as `Reset` so the terminal's own palette
    /// is used (follows light/dark terminal themes without guessing).
    pub fn auto() -> Self {
        Self {
            mode: "auto".into(),
            primary: Color::Reset,
            gray: Color::Reset,
            background: Color::Reset,
            dim_secondary: true,
            inverse_button: true,
        }
    }

    pub fn from_mode(mode: &str) -> Self {
        let core: CoreTheme = theme_for(mode);
        Self {
            mode: core.mode,
            primary: core
                .primary
                .as_deref()
                .and_then(parse_hex)
                .unwrap_or(Color::Reset),
            gray: core
                .gray
                .as_deref()
                .and_then(parse_hex)
                .unwrap_or(Color::Reset),
            background: core
                .background
                .as_deref()
                .and_then(parse_hex)
                .unwrap_or(Color::Reset),
            dim_secondary: core.dim_secondary,
            inverse_button: core.inverse_button,
        }
    }

    pub fn primary_style(&self) -> Style {
        Style::default().fg(self.primary)
    }

    pub fn gray_style(&self) -> Style {
        let mut s = Style::default().fg(self.gray);
        if self.dim_secondary {
            s = s.add_modifier(Modifier::DIM);
        }
        s
    }
}

fn parse_hex(hex: &str) -> Option<Color> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

pub fn tui_theme_for(mode: ThemeMode) -> TuiTheme {
    TuiTheme::from_mode(mode.as_str())
}
