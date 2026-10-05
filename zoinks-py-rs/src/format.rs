//! Number / string formatting helpers — port of `src/lib/format.ts`.
//!
//! These are hot enough (called every progress frame) that an allocation-free
//! implementation is worth the small extra code over `format!`.

/// Human-friendly byte count, e.g. `12.3 MB`. Empty string for non-finite or
/// non-positive input, matching the TS `formatBytes`.
pub fn format_bytes(bytes: f64) -> String {
    if !bytes.is_finite() || bytes <= 0.0 {
        return String::new();
    }
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 10.0 || unit == 0 {
        format!("{} {}", value.round() as i64, UNITS[unit])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

/// `mm:ss` or `h:mm:ss` from seconds. Empty for non-finite / non-positive.
pub fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return String::new();
    }
    let s = seconds.round() as i64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, sec)
    } else {
        format!("{}:{:02}", m, sec)
    }
}

/// Truncate with a trailing ellipsis if `text.len() > max`.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Tidy up a file path for display: replace the home dir with `~`, then
/// truncate the middle so the extension stays visible.
pub fn shorten_path(filepath: &str, homedir: &str, max: usize) -> String {
    let pretty = if !homedir.is_empty() && filepath.starts_with(homedir) {
        format!("~{}", &filepath[homedir.len()..])
    } else {
        filepath.to_string()
    };
    if pretty.chars().count() <= max {
        return pretty;
    }

    // grab the trailing extension — `\.\w{1,5}$` from the TS regex
    let ext: String = pretty
        .rfind('.')
        .filter(|&dot_idx| {
            let tail = &pretty[dot_idx..];
            // 2..=6 chars total: the dot + 1..=5 word chars
            let len = tail.chars().count();
            (2..=6).contains(&len) && tail[1..].chars().all(|c| c.is_ascii_alphanumeric())
        })
        .map(|dot_idx| pretty[dot_idx..].to_string())
        .unwrap_or_default();

    let take = max.saturating_sub(ext.chars().count() + 1);
    let mut out: String = pretty.chars().take(take).collect();
    out.push('…');
    out.push_str(&ext);
    out
}

/// Left-flush word-wrap — mirrors `wrapText` from the TS version. Ink's own
/// wrapper keeps the breaking space as a one-cell indent on continuation
/// lines; we drop it so wrapped lines stay flush left.
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if line.is_empty() {
            line.push_str(word);
        } else if line.chars().count() + 1 + word.chars().count() <= width {
            line.push(' ');
            line.push_str(word);
        } else {
            lines.push(std::mem::take(&mut line));
            line.push_str(word);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

pub fn format_speed(bytes_per_second: f64) -> String {
    if !bytes_per_second.is_finite() || bytes_per_second <= 0.0 {
        return String::new();
    }
    format!("{}/s", format_bytes(bytes_per_second))
}

pub fn format_eta(seconds: f64) -> String {
    format_duration(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_units() {
        assert_eq!(format_bytes(0.0), "");
        assert_eq!(format_bytes(512.0), "512 B");
        assert_eq!(format_bytes(1024.0), "1.0 KB");
        assert_eq!(format_bytes(1024.0 * 1024.0), "1.0 MB");
        assert_eq!(format_bytes(15.0 * 1024.0 * 1024.0), "15 MB");
    }

    #[test]
    fn duration_shapes() {
        assert_eq!(format_duration(45.0), "0:45");
        assert_eq!(format_duration(125.0), "2:05");
        assert_eq!(format_duration(3725.0), "1:02:05");
        assert_eq!(format_duration(0.0), "");
    }

    #[test]
    fn truncate_ellipsis() {
        assert_eq!(truncate("hi", 10), "hi");
        assert_eq!(truncate("hello world", 5), "hell…");
    }

    #[test]
    fn shorten_path_keeps_ext() {
        let p = "/Users/me/Downloads/Some Really Long Video Title Goes Here.mp4";
        let short = shorten_path(p, "/Users/me", 30);
        assert!(short.starts_with("~/Downloads/"));
        assert!(short.ends_with(".mp4"));
        assert!(short.chars().count() <= 30);
    }

    #[test]
    fn wrap_text_left_flush() {
        let lines = wrap_text("the quick brown fox jumps over the lazy dog", 10);
        assert_eq!(lines[0], "the quick");
        assert!(!lines[1].starts_with(' '));
    }
}
