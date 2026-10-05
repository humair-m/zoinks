//! zoinks core — a faithful Rust port of the TypeScript `yoinks` TUI by Pablo Stanley.
//!
//! # Attribution
//!
//! `zoinks` is a Python + Rust port of Pablo Stanley's `yoinks` TypeScript TUI
//! (https://github.com/pablostanley/yoinks, MIT-licensed). All credit for the
//! original design, tagline, block-character logo, UX flow, and theme palette
//! belongs to Pablo Stanley. The original MIT license is preserved.
//!
//! # Surfaces
//!
//! This crate exposes two surfaces:
//!   * a Python module via PyO3 (see [`py`])
//!   * a standalone ratatui-based TUI binary (see `src/bin/zoinks_tui.rs`)
//!
//! All platform-specific I/O (yt-dlp, ffmpeg, clipboard, history, download dir)
//! lives in [`crate::ytdlp`], [`crate::clipboard`], [`crate::history`] — and is
//! shared by both surfaces so behaviour stays in lock-step.

pub mod args;
pub mod clipboard;
pub mod format;
pub mod history;
pub mod platforms;
pub mod theme;
pub mod tui;
pub mod ytdlp;

#[cfg(feature = "python")]
pub mod py;

pub use args::{parse_args, CliArgs};
pub use clipboard::read_clipboard;
pub use format::*;
pub use history::{add_to_history, load_history};
pub use platforms::{detect_platform, is_probably_url, Platform};
pub use theme::{theme_for, Theme, ThemeMode, THEME_MODES};
pub use ytdlp::{
    build_choices, download, ensure_yt_dlp, find_ffmpeg, probe, DownloadChoice,
    DownloadHandlers, DownloadProgress, ProbeResult, VideoInfo,
};

/// Crate-wide version string. Kept in sync with `Cargo.toml` and `pyproject.toml`
/// by the release checklist in `CONTRIBUTING.md`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The directory under the user's home where zoinks stores its bundled yt-dlp
/// binary and any per-user state. Mirrors `~/.zoinks/bin` from the TS version.
pub fn zoinks_bin_dir() -> std::path::PathBuf {
    let home = directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    home.join(".zoinks").join("bin")
}

/// Default download destination — same as the TS app: `~/Downloads`.
pub fn default_out_dir() -> std::path::PathBuf {
    directories::UserDirs::new()
        .and_then(|d| d.download_dir().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| {
            directories::BaseDirs::new()
                .map(|b| b.home_dir().join("Downloads"))
                .unwrap_or_else(|| std::path::PathBuf::from("."))
        })
}
