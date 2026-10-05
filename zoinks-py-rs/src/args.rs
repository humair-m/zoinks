//! CLI argument parsing — port of `src/lib/args.ts`.
//!
//! The shape is intentionally a 1:1 mirror of the TS version so behaviour
//! (and tests) translate directly. We hand-roll the parser rather than pulling
//! in `clap` so the public surface stays minimal and the error strings match.

use crate::theme::ThemeMode;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliArgs {
    pub help: bool,
    pub version: bool,
    pub update: bool,
    pub initial_url: Option<String>,
    pub theme_mode: Option<ThemeMode>,
    pub cookies: Option<String>,
    pub cookies_from_browser: Option<String>,
    pub error: Option<String>,
}

/// Parse `argv` (everything after the program name). Equivalent to the TS
/// `parseArgs` — same option set, same error messages, same precedence.
pub fn parse_args(args: &[String]) -> CliArgs {
    let mut result = CliArgs::default();
    let mut positional: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        if arg == "-h" || arg == "--help" {
            result.help = true;
        } else if arg == "-v" || arg == "--version" {
            result.version = true;
        } else if arg == "--update" || arg == "-U" {
            result.update = true;
        } else if arg == "--cookies" {
            i += 1;
            let Some(value) = args.get(i) else {
                return CliArgs {
                    error: Some("--cookies needs a value: path to a Netscape-format cookies file".to_string()),
                    ..result
                };
            };
            result.cookies = Some(value.clone());
        } else if let Some(value) = arg.strip_prefix("--cookies=") {
            result.cookies = Some(value.to_string());
        } else if arg == "--cookies-from-browser" {
            i += 1;
            let Some(value) = args.get(i) else {
                return CliArgs {
                    error: Some("--cookies-from-browser needs a value: chrome, firefox, safari, edge, opera, chromium, brave, vivaldi, or whale".to_string()),
                    ..result
                };
            };
            result.cookies_from_browser = Some(value.clone());
        } else if let Some(value) = arg.strip_prefix("--cookies-from-browser=") {
            result.cookies_from_browser = Some(value.to_string());
        } else if arg == "--theme" {
            i += 1;
            let Some(value) = args.get(i) else {
                return CliArgs {
                    error: Some("--theme needs a value: auto, light, or dark".to_string()),
                    ..result
                };
            };
            match ThemeMode::from_str(value) {
                Some(mode) => result.theme_mode = Some(mode),
                None => {
                    return CliArgs {
                        error: Some(format!(
                            "unknown theme “{value}” — use auto, light, or dark"
                        )),
                        ..result
                    };
                }
            }
        } else if let Some(value) = arg.strip_prefix("--theme=") {
            match ThemeMode::from_str(value) {
                Some(mode) => result.theme_mode = Some(mode),
                None => {
                    return CliArgs {
                        error: Some(format!(
                            "unknown theme “{value}” — use auto, light, or dark"
                        )),
                        ..result
                    };
                }
            }
        } else if arg.starts_with('-') {
            return CliArgs {
                error: Some(format!("unknown option “{arg}”")),
                ..result
            };
        } else {
            positional.push(arg.clone());
        }

        i += 1;
    }

    if positional.len() > 1 {
        return CliArgs {
            error: Some("expected a single url".to_string()),
            ..result
        };
    }
    result.initial_url = positional.into_iter().next();
    result
}

/// The `--help` text — kept here so both the Rust binary and the Python CLI
/// can print the same string without one drifting from the other.
pub const HELP: &str = "\
  zoinks — zoink any video. paste. zoink. done.

  Usage
    $ zoinks [url]

  Examples
    $ zoinks https://youtu.be/dQw4w9WgXcQ
    $ zoinks https://x.com/user/status/123456
    $ zoinks                                   (prompts for a url)
    $ zoinks --cookies-from-browser chrome <url>  (auto-pull cookies from Chrome)
    $ zoinks --cookies cookies.txt <url>           (Netscape-format cookies file)
    $ zoinks --update                             (update bundled yt-dlp)

  Options
    --theme <mode>            use auto, light, or dark for this run
    --cookies-from-browser <b>  pass cookies from chrome/firefox/safari/edge/
                              opera/chromium/brave/vivaldi/whale to yt-dlp
    --cookies <path>         pass a Netscape-format cookies file to yt-dlp
    -U, --update             update the bundled yt-dlp to the latest release
    -h, --help               show this help
    -v, --version            show version

  Downloads are saved to ~/Downloads.
  Powered by yt-dlp — YouTube, X, Instagram, Threads, TikTok & 1800+ sites.

  A port of Pablo Stanley's `yoinks` (https://github.com/pablostanley/yoinks).";

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn help_short_and_long() {
        assert!(parse_args(&args(&["-h"])).help);
        assert!(parse_args(&args(&["--help"])).help);
    }

    #[test]
    fn version_short_and_long() {
        assert!(parse_args(&args(&["-v"])).version);
        assert!(parse_args(&args(&["--version"])).version);
    }

    #[test]
    fn positional_url() {
        let parsed = parse_args(&args(&["https://youtu.be/x"]));
        assert_eq!(parsed.initial_url.as_deref(), Some("https://youtu.be/x"));
        assert!(parsed.error.is_none());
    }

    #[test]
    fn too_many_urls_errors() {
        let parsed = parse_args(&args(&["https://a", "https://b"]));
        assert_eq!(parsed.error.as_deref(), Some("expected a single url"));
    }

    #[test]
    fn theme_value_and_eq_form() {
        assert_eq!(
            parse_args(&args(&["--theme", "dark"])).theme_mode,
            Some(ThemeMode::Dark)
        );
        assert_eq!(
            parse_args(&args(&["--theme=light"])).theme_mode,
            Some(ThemeMode::Light)
        );
    }

    #[test]
    fn unknown_theme_errors() {
        let parsed = parse_args(&args(&["--theme", "neon"]));
        assert!(parsed.error.is_some());
    }

    #[test]
    fn unknown_flag_errors() {
        let parsed = parse_args(&args(&["--nope"]));
        assert!(parsed.error.as_ref().unwrap().contains("unknown option"));
    }
}
