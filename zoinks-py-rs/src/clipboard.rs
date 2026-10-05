//! Clipboard reader — port of `src/lib/clipboard.ts`.
//!
//! Tries each platform's native clipboard utility in turn; returns `""` if
//! none is available or the clipboard is empty. The caller decides whether
//! the contents look like a URL.

use std::process::Command;

fn clipboard_commands() -> Vec<(&'static str, Vec<&'static str>)> {
    #[cfg(target_os = "macos")]
    {
        vec![("pbpaste", vec![])]
    }
    #[cfg(target_os = "windows")]
    {
        vec![(
            "powershell",
            vec!["-NoProfile", "-Command", "Get-Clipboard"],
        )]
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        vec![
            ("wl-paste", vec!["--no-newline"]),
            ("xclip", vec!["-selection", "clipboard", "-o"]),
            ("xsel", vec!["--clipboard", "--output"]),
        ]
    }
}

/// Read the system clipboard. Returns an empty string on any failure — the
/// caller is responsible for deciding whether the contents look like a URL.
pub fn read_clipboard() -> String {
    for (cmd, args) in clipboard_commands() {
        let result = Command::new(cmd)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();

        if let Ok(output) = result {
            if output.status.success() {
                return String::from_utf8_lossy(&output.stdout).into_owned();
            }
        }
    }
    String::new()
}
