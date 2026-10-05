//! Embedded yt-dlp + ffmpeg binaries.
//!
//! When the `bundled` feature is on, the standalone yt-dlp binary and ffmpeg
//! are baked into the zoinks binary via `include_bytes!`. On first run they
//! are extracted to `~/.zoinks/bin/` and chmod'd executable. This makes the
//! Rust binary completely self-contained — no first-run download, no Python
//! required, no system yt-dlp/ffmpeg needed.
//!
//! The cost is binary size: embedding adds ~95 MB (yt-dlp 39 MB + ffmpeg 58 MB).
//! For headless / offline / restricted-network deployments this is the right
//! trade-off. For interactive use, users can still `--update` to refresh the
//! bundled yt-dlp to the latest release.
//!
//! When the `bundled` feature is OFF (default for `cargo build`), the binary
//! behaves exactly like before: it falls back to a system yt-dlp if present,
//! and downloads yt-dlp on first run to `~/.zoinks/bin`.

use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::zoinks_bin_dir;

#[cfg(feature = "bundled")]
const YT_DLP_BYTES: &[u8] = include_bytes!("../bundled/yt-dlp");
#[cfg(feature = "bundled")]
const FFMPEG_BYTES: &[u8] = include_bytes!("../bundled/ffmpeg");

/// Write the embedded yt-dlp binary to `~/.zoinks/bin/yt-dlp` if it's not
/// already there. Returns the path on success.
///
/// When `bundled` is off, this is a no-op that returns `None` — the caller
/// is expected to fall back to `ensure_yt_dlp` (download on first run).
pub fn extract_embedded_yt_dlp() -> Option<PathBuf> {
    #[cfg(feature = "bundled")]
    {
        // Skip if the embedded bytes are empty (e.g., the build.rs wrote an
        // empty placeholder because there's no static build for this platform).
        if YT_DLP_BYTES.is_empty() {
            return None;
        }
        let bin_name = if cfg!(target_os = "windows") {
            "yt-dlp.exe"
        } else {
            "yt-dlp"
        };
        let dest = zoinks_bin_dir().join(bin_name);

        // If the file already exists and matches the embedded size, skip the
        // extraction. (We use size as a cheap checksum — if the user runs
        // `--update`, the new yt-dlp will have a different size and we'll
        // re-extract on the next launch.)
        if let Ok(meta) = fs::metadata(&dest) {
            if meta.len() as usize == YT_DLP_BYTES.len() {
                return Some(dest);
            }
        }

        let _ = fs::create_dir_all(zoinks_bin_dir());
        let tmp = dest.with_extension("extract");
        if fs::write(&tmp, YT_DLP_BYTES).is_err() {
            return None;
        }
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&tmp).ok()?.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&tmp, perms);
        }
        if fs::rename(&tmp, &dest).is_err() {
            let _ = fs::remove_file(&tmp);
            return None;
        }
        Some(dest)
    }
    #[cfg(not(feature = "bundled"))]
    {
        None
    }
}

/// Write the embedded ffmpeg binary to `~/.zoinks/bin/ffmpeg` if it's not
/// already there. Returns the path on success.
pub fn extract_embedded_ffmpeg() -> Option<PathBuf> {
    #[cfg(feature = "bundled")]
    {
        // Skip if empty (macOS case — no static build available, build.rs
        // wrote an empty placeholder).
        if FFMPEG_BYTES.is_empty() {
            return None;
        }
        let bin_name = if cfg!(target_os = "windows") {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        let dest = zoinks_bin_dir().join(bin_name);

        if let Ok(meta) = fs::metadata(&dest) {
            if meta.len() as usize == FFMPEG_BYTES.len() {
                return Some(dest);
            }
        }

        let _ = fs::create_dir_all(zoinks_bin_dir());
        let tmp = dest.with_extension("extract");
        if fs::write(&tmp, FFMPEG_BYTES).is_err() {
            return None;
        }
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&tmp).ok()?.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&tmp, perms);
        }
        if fs::rename(&tmp, &dest).is_err() {
            let _ = fs::remove_file(&tmp);
            return None;
        }
        Some(dest)
    }
    #[cfg(not(feature = "bundled"))]
    {
        None
    }
}
