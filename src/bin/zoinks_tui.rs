//! `zoinks-tui` — standalone Rust TUI binary. No Python required.
//!
//! Mirrors the original TS `cli.tsx` flow: parse args, enter alternate
//! screen, run the [`crate::tui::app`] event loop, restore the terminal on
//! exit, print the saved file path.
//!
//! This is a port of Pablo Stanley's `yoinks` (https://github.com/pablostanley/yoinks).
//! All credit for the design, tagline, and the block-character logo goes to
//! him. The MIT license is preserved.

use std::path::Path;
use std::process::ExitCode;

use zoinks::args::{parse_args, HELP};
use zoinks::tui::app;
use zoinks::theme::THEME_MODES;
use zoinks::ytdlp::{ensure_yt_dlp, update_yt_dlp};
use zoinks::VERSION;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let parsed = parse_args(&argv);

    if let Some(err) = &parsed.error {
        eprintln!("zoinks: {err}\nTry “zoinks --help” for usage.");
        return ExitCode::from(1);
    }
    if parsed.help {
        println!("{HELP}");
        return ExitCode::SUCCESS;
    }
    if parsed.version {
        println!("{VERSION}");
        return ExitCode::SUCCESS;
    }

    let theme_mode = parsed
        .theme_mode
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "auto".into());
    if !THEME_MODES.iter().any(|m| m.as_str() == theme_mode) {
        eprintln!("zoinks: unknown theme “{theme_mode}” — use auto, light, or dark");
        return ExitCode::from(1);
    }

    // `--update` short-circuits: ensure yt-dlp exists, then run `yt-dlp -U`,
    // then exit. Useful when the user hits an "Unsupported URL" error and
    // wants to refresh the bundled yt-dlp without going through the TUI.
    if parsed.update {
        let aborted = std::sync::atomic::AtomicBool::new(false);
        let mut on_status = |msg: &str| eprintln!("zoinks: {msg}");
        match ensure_yt_dlp(&mut on_status, &aborted) {
            Ok(path) => {
                eprintln!("zoinks: yt-dlp at {path}");
                match update_yt_dlp(&path, &mut on_status) {
                    Ok(()) => {
                        eprintln!("zoinks: yt-dlp updated.");
                        return ExitCode::SUCCESS;
                    }
                    Err(e) => {
                        eprintln!("zoinks: update failed: {e}");
                        return ExitCode::from(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("zoinks: {e}");
                return ExitCode::from(1);
            }
        }
    }

    let url = parsed.initial_url.as_deref();
    let cookies = parsed.cookies.as_deref().map(Path::new);
    match app::run_with_cookies(url, &theme_mode, cookies) {
        Ok(Some(filepath)) => {
            println!("✓ yoinked → {filepath}");
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zoinks: {e}");
            ExitCode::from(1)
        }
    }
}
