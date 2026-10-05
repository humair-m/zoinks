//! Theme palette — port of `src/theme.ts`.
//!
//! `auto` leaves colors unset so the terminal's own palette is used; this
//! follows light/dark terminal themes without guessing. `light` and `dark`
//! pin explicit hex values so the picker stays legible regardless of the
//! host terminal.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Auto,
    Light,
    Dark,
}

pub const THEME_MODES: &[ThemeMode] = &[ThemeMode::Auto, ThemeMode::Dark, ThemeMode::Light];

impl ThemeMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThemeMode::Auto => "auto",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(ThemeMode::Auto),
            "light" => Some(ThemeMode::Light),
            "dark" => Some(ThemeMode::Dark),
            _ => None,
        }
    }
}

impl std::fmt::Display for ThemeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn is_theme_mode(value: &str) -> bool {
    ThemeMode::from_str(value).is_some()
}

pub fn theme_for(mode: &str) -> Theme {
    match ThemeMode::from_str(mode) {
        Some(ThemeMode::Light) => Theme::light(),
        Some(ThemeMode::Dark) => Theme::dark(),
        _ => Theme::auto(),
    }
}

pub fn next_theme_mode(mode: ThemeMode) -> ThemeMode {
    let idx = THEME_MODES.iter().position(|m| *m == mode).unwrap_or(0);
    THEME_MODES[(idx + 1) % THEME_MODES.len()]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub mode: String,
    pub primary: Option<String>,
    pub gray: Option<String>,
    pub dark: Option<String>,
    pub background: Option<String>,
    pub dim_secondary: bool,
    pub inverse_button: bool,
}

impl Theme {
    pub fn auto() -> Self {
        Self {
            mode: "auto".into(),
            primary: None,
            gray: None,
            dark: None,
            background: None,
            dim_secondary: true,
            inverse_button: true,
        }
    }

    pub fn light() -> Self {
        Self {
            mode: "light".into(),
            primary: Some("#18181b".into()),
            gray: Some("#52525b".into()),
            dark: Some("#ffffff".into()),
            background: Some("#ffffff".into()),
            dim_secondary: false,
            inverse_button: false,
        }
    }

    pub fn dark() -> Self {
        Self {
            mode: "dark".into(),
            primary: Some("#ffffff".into()),
            gray: Some("#a1a1aa".into()),
            dark: Some("#18181b".into()),
            background: Some("#18181b".into()),
            dim_secondary: false,
            inverse_button: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_through_modes() {
        let mut m = THEME_MODES[0];
        for _ in 0..3 {
            m = next_theme_mode(m);
        }
        assert_eq!(m, ThemeMode::Auto);
    }

    #[test]
    fn theme_for_returns_correct_palette() {
        assert_eq!(theme_for("dark").primary.as_deref(), Some("#ffffff"));
        assert_eq!(theme_for("light").background.as_deref(), Some("#ffffff"));
        assert!(theme_for("auto").primary.is_none());
    }
}
