//! Platform detection & URL validation — port of `src/lib/platforms.ts`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    pub key: String,
    pub label: String,
}

impl Platform {
    pub fn new(key: &str, label: &str) -> Self {
        Self {
            key: key.to_string(),
            label: label.to_string(),
        }
    }
}

/// Static platform table — kept separate from the owned [`Platform`] so the
/// const array doesn't need `String` allocation.
const PLATFORMS: &[(&[&str], &str, &str)] = &[
    (&["youtube.com", "youtu.be", "music.youtube.com"], "youtube", "YouTube"),
    (&["x.com", "twitter.com"], "x", "X / Twitter"),
    (&["instagram.com"], "instagram", "Instagram"),
    (&["threads.net", "threads.com"], "threads", "Threads"),
    (&["tiktok.com"], "tiktok", "TikTok"),
    (&["vimeo.com"], "vimeo", "Vimeo"),
    (&["twitch.tv"], "twitch", "Twitch"),
    (&["reddit.com"], "reddit", "Reddit"),
    (&["facebook.com", "fb.watch"], "facebook", "Facebook"),
];

/// Mirror of `detectPlatform` — host match, returns `unknown` on parse error
/// and `generic` (with the host as label) when no platform matches.
pub fn detect_platform(url: &str) -> Platform {
    let hostname = match parse_hostname(url) {
        Some(h) => h.to_lowercase(),
        None => return Platform::new("unknown", "Unknown site"),
    };

    for (hosts, key, label) in PLATFORMS {
        if hosts
            .iter()
            .any(|h| hostname == *h || hostname.ends_with(&format!(".{h}")))
        {
            return Platform::new(key, label);
        }
    }

    Platform::new("generic", &hostname)
}

/// True iff `input` parses as an http/https URL. Mirrors `isProbablyUrl` —
/// uses a lightweight scheme check rather than pulling in a full URL parser.
pub fn is_probably_url(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }
    let mut parts = trimmed.split("://");
    let scheme = parts.next().unwrap_or("");
    let after_scheme = parts.next();
    if scheme != "http" && scheme != "https" {
        return false;
    }
    let Some(after) = after_scheme else {
        return false;
    };
    // after must have at least one valid host char before any /, ?, #
    let authority = after
        .split(|c: char| c == '/' || c == '?' || c == '#')
        .next()
        .unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

fn parse_hostname(url: &str) -> Option<&str> {
    // require a scheme — without one the input isn't a URL and we'd return
    // "unknown" downstream by accident (mirrors `new URL()` throwing)
    let after_scheme = url.split("://").nth(1)?;
    let authority = after_scheme
        .split(|c: char| c == '/' || c == '?' || c == '#')
        .next()?;
    let host = authority.rsplit('@').next()?;
    let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_platforms() {
        assert_eq!(detect_platform("https://youtu.be/abc").key, "youtube");
        assert_eq!(detect_platform("https://x.com/u/status/1").key, "x");
        assert_eq!(detect_platform("https://www.tiktok.com/@u").key, "tiktok");
    }

    #[test]
    fn unknown_url_is_generic() {
        let p = detect_platform("https://example.com/v/1");
        assert_eq!(p.key, "generic");
        assert!(p.label.contains("example.com"));
    }

    #[test]
    fn garbage_is_unknown() {
        assert_eq!(detect_platform("not a url").key, "unknown");
    }

    #[test]
    fn is_url_validates_scheme() {
        assert!(is_probably_url("https://youtu.be/x"));
        assert!(is_probably_url("http://example.com"));
        assert!(!is_probably_url("not a url"));
        assert!(!is_probably_url("ftp://example.com"));
    }
}
