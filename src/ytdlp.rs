//! yt-dlp + ffmpeg wrapper — port of `src/lib/ytdlp.ts`.
//!
//! Resolves the standalone yt-dlp binary, probes a URL for media metadata,
//! builds the resolution-picker choices, and downloads with progress
//! streaming. All blocking work is dispatched on a background thread so the
//! caller (a TUI loop) can stay responsive.
//!
//! The progress wire-format matches the TS version's `YOINK|…` template, so
//! behaviour is identical: each `%(progress.downloaded_bytes)s` is parsed,
//! parts are counted by detecting a reset of `downloaded_bytes`, and a final
//! `after_move:filepath` print line gives us the saved location.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::format::format_bytes;
use crate::zoinks_bin_dir;

const RELEASE_BASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoInfo {
    pub title: String,
    #[serde(default)]
    pub uploader: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub webpage_url: Option<String>,
    #[serde(default)]
    pub extractor_key: Option<String>,
    #[serde(default)]
    pub formats: Vec<RawFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawFormat {
    pub format_id: String,
    #[serde(default)]
    pub ext: Option<String>,
    #[serde(default)]
    pub vcodec: Option<String>,
    #[serde(default)]
    pub acodec: Option<String>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub abr: Option<f64>,
    #[serde(default)]
    pub tbr: Option<f64>,
    #[serde(default)]
    pub filesize: Option<u64>,
    #[serde(default)]
    pub filesize_approx: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub info: VideoInfo,
    /// Path to a temp file holding the raw `-J` output, so a subsequent
    /// `download` call can pass `--load-info-json` and skip re-extraction.
    pub info_json_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadChoice {
    pub label: String,
    pub kind: ChoiceKind,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChoiceKind {
    Video,
    Audio,
}

#[derive(Debug, Clone, Default)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub speed: Option<f64>,
    pub eta: Option<f64>,
    pub part: u32,
    pub total_parts: u32,
}

/// Callbacks the download loop drives — same shape as the TS `DownloadHandlers`.
/// Both are called from the IO thread; the consumer is expected to forward
/// state into its own UI thread (see how `tui::App` does it via channels).
pub struct DownloadHandlers {
    pub on_progress: Box<dyn FnMut(DownloadProgress) + Send>,
    pub on_processing: Box<dyn FnMut() + Send>,
}

/// Cancellation handle for an in-flight download. Drop the [`DownloadGuard`]
/// to abort the spawned child.
pub struct DownloadGuard {
    child: Arc<Mutex<Option<Child>>>,
    aborted: Arc<AtomicBool>,
}

impl DownloadGuard {
    /// Abort the underlying yt-dlp child if it's still running.
    pub fn abort(&self) {
        self.aborted.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            *guard = None;
        }
    }
}

impl Drop for DownloadGuard {
    fn drop(&mut self) {
        self.abort();
    }
}

fn yt_dlp_asset_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "yt-dlp.exe"
    }
    #[cfg(target_os = "macos")]
    {
        "yt-dlp_macos"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if cfg!(target_arch = "aarch64") {
            "yt-dlp_linux_aarch64"
        } else {
            "yt-dlp_linux"
        }
    }
}

/// Run a command with stdin/stdout/stderr silenced. Returns true if it
/// exited cleanly. Used to verify whether `yt-dlp` / `ffmpeg` are on PATH.
fn command_works(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Resolve a usable yt-dlp: system install first, then a previously
/// downloaded copy in `~/.zoinks/bin`, then fetch the standalone binary
/// from GitHub releases.
///
/// `on_status` is called (synchronously) with one-line human-readable status
/// strings — the TUI prints these under its spinner.
pub fn ensure_yt_dlp(
    on_status: &mut dyn FnMut(&str),
    aborted: &AtomicBool,
) -> Result<String, String> {
    // Prefer the system yt-dlp if it's recent enough. Older system installs
    // (Debian's `python3-yt-dlp` is often a year out of date) silently fail
    // on modern X snowflake IDs, FB reels, etc., so we always check the
    // version string. If it predates 2024 we use our own managed copy.
    if let Some(path) = system_yt_dlp_if_recent() {
        return Ok(path);
    }

    let bin_name = if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    };
    let local = zoinks_bin_dir().join(bin_name);

    // If we have a managed copy and it's recent enough, use it
    if let Some(path) = managed_yt_dlp_if_recent(&local) {
        return Ok(path);
    }

    // Otherwise, download (or re-download) the latest release
    on_status("first run: fetching yt-dlp…");
    fs::create_dir_all(zoinks_bin_dir()).map_err(|e| format!("Could not create bin dir: {e}"))?;

    let url = format!("{}/{}", RELEASE_BASE, yt_dlp_asset_name());
    let response = ureq::get(&url)
        .timeout(Duration::from_secs(120))
        .call()
        .map_err(|e| format!("Could not download yt-dlp ({e}). Check your connection and try again."))?;

    let tmp = local.with_extension("download");
    {
        let mut file = fs::File::create(&tmp).map_err(|e| format!("Could not create temp file: {e}"))?;
        let mut reader = response.into_reader();
        std::io::copy(&mut reader, &mut file).map_err(|e| format!("Download failed: {e}"))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&tmp).map_err(|e| e.to_string())?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tmp, perms).map_err(|e| e.to_string())?;
    }

    if aborted.load(Ordering::SeqCst) {
        let _ = fs::remove_file(&tmp);
        return Err("aborted".into());
    }

    fs::rename(&tmp, &local).map_err(|e| format!("Could not install yt-dlp: {e}"))?;
    Ok(local.to_string_lossy().into_owned())
}

/// Run `yt-dlp -U` against a managed install to update it in place. Best
/// effort — returns Ok(()) even if the update fails (the next probe will
/// still try with whatever version is on disk).
pub fn update_yt_dlp(ytdlp_path: &str, on_status: &mut dyn FnMut(&str)) -> Result<(), String> {
    on_status("updating yt-dlp…");
    let status = Command::new(ytdlp_path)
        .arg("-U")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("Could not run yt-dlp -U: {e}"))?;
    if !status.success() {
        // Self-update may be disabled for the standalone binary on some
        // platforms — re-download instead.
        on_status("self-update unavailable — re-downloading latest…");
        let aborted = AtomicBool::new(false);
        let _ = ensure_yt_dlp(on_status, &aborted);
    }
    Ok(())
}

/// Check the system PATH for `yt-dlp` and return its path iff its version
/// is at least 2024.0. Older versions don't recognise modern X snowflake
/// IDs, FB reel URLs, etc. — we'd rather re-download a fresh copy than
/// use one that will silently fail.
fn system_yt_dlp_if_recent() -> Option<String> {
    let output = Command::new("yt-dlp")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout);
    let version = version.lines().next()?.trim();
    if yt_dlp_version_is_recent(version) {
        Some("yt-dlp".into())
    } else {
        None
    }
}

fn managed_yt_dlp_if_recent(local: &Path) -> Option<String> {
    let output = Command::new(local)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout);
    let version = version.lines().next()?.trim();
    if yt_dlp_version_is_recent(version) {
        Some(local.to_string_lossy().into_owned())
    } else {
        None
    }
}

/// True iff the yt-dlp version string is recent enough for the modern URL
/// patterns we care about. yt-dlp stamps versions as `YYYY.MM.DD` (with
/// optional `.<patch>`); anything in 2024 or later is good enough.
fn yt_dlp_version_is_recent(version: &str) -> bool {
    // accept YYYY.MM.DD or YYYY.MM.DD.NN — extract the year
    let year_str = version.split('.').next().unwrap_or("");
    year_str.parse::<u32>().map(|y| y >= 2024).unwrap_or(false)
}

/// Find ffmpeg for stream merging / mp3 extraction: system PATH first.
/// Returns `None` if no usable ffmpeg is found — yt-dlp still works for
/// single-file formats without it.
pub fn find_ffmpeg() -> Option<String> {
    if command_works("ffmpeg", &["-version"]) {
        return None; // on PATH, yt-dlp finds it itself
    }
    which::which("ffmpeg").ok().map(|p| p.to_string_lossy().into_owned())
}

/// Probe a URL for media info. Spawns `yt-dlp -J`, writes the raw JSON to a
/// temp file (so the download call can `--load-info-json` it), parses it
/// into [`VideoInfo`]. `aborted` is polled between major steps.
///
/// `cookies` is an optional path to a Netscape-format cookies file — pass
/// it for sites that require authentication (X/Twitter, Facebook, etc.).
pub fn probe(
    ytdlp: &str,
    url: &str,
    cookies: Option<&Path>,
    aborted: &AtomicBool,
) -> Result<ProbeResult, String> {
    let mut cmd = Command::new(ytdlp);
    cmd.args(["-J", "--no-playlist", "--no-warnings"]);
    if let Some(cookies_path) = cookies {
        cmd.args(["--cookies", &cookies_path.to_string_lossy()]);
    }
    cmd.arg(url);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| format!("Could not run yt-dlp: {e}"))?;

    let output = wait_with_abort(child, aborted)?;
    if !output.status.success() {
        return Err(clean_yt_dlp_error(&String::from_utf8_lossy(&output.stderr))
            .unwrap_or_else(|| {
                format!("yt-dlp exited with code {}", output.status.code().unwrap_or(-1))
            }));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let info: VideoInfo = serde_json::from_str(&stdout)
        .map_err(|_| "Could not parse video info from yt-dlp.".to_string())?;

    let info_json_path = std::env::temp_dir().join(format!(
        "zoinks-info-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    fs::write(&info_json_path, stdout.as_bytes())
        .map_err(|e| format!("Could not cache video info: {e}"))?;

    Ok(ProbeResult { info, info_json_path })
}

// keep wait_with_abort near probe since only probe uses it

/// Build the picker choices from a probe result — one per distinct video
/// height (capped at 8) plus an audio-only mp3. Mirrors `buildChoices`.
pub fn build_choices(info: &VideoInfo) -> Vec<DownloadChoice> {
    let formats = &info.formats;
    let mut choices: Vec<DownloadChoice> = Vec::new();

    // best audio track — used both for size math and as the audio leg of
    // a video+audio merge
    let audio_only: Vec<&RawFormat> = formats
        .iter()
        .filter(|f| {
            f.acodec.as_deref().is_some_and(|c| c != "none")
                && f.vcodec.as_deref().map_or(true, |c| c == "none")
        })
        .collect();
    let best_audio = audio_only
        .into_iter()
        .max_by(|a, b| {
            let av = a.abr.unwrap_or(a.tbr.unwrap_or(0.0));
            let bv = b.abr.unwrap_or(b.tbr.unwrap_or(0.0));
            av.partial_cmp(&bv).unwrap_or(std::cmp::Ordering::Equal)
        });
    let audio_size = best_audio.and_then(|f| f.filesize.or(f.filesize_approx));

    let videos: Vec<&RawFormat> = formats
        .iter()
        .filter(|f| {
            f.vcodec.as_deref().is_some_and(|c| c != "none") && f.height.is_some()
        })
        .collect();

    let mut heights: Vec<u32> = videos.iter().filter_map(|f| f.height).collect();
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();

    const MAX_VIDEO_CHOICES: usize = 8;
    for height in heights.iter().take(MAX_VIDEO_CHOICES) {
        let candidates: Vec<&&RawFormat> = videos.iter().filter(|f| f.height == Some(*height)).collect();
        let best = candidates
            .into_iter()
            .max_by(|a, b| score_video(a).partial_cmp(&score_video(b)).unwrap_or(std::cmp::Ordering::Equal));
        let Some(best) = best else { continue };
        let muxed = best.acodec.as_deref().is_some_and(|c| c != "none");
        let size = best.filesize.or(best.filesize_approx).unwrap_or(0)
            + if muxed { 0 } else { audio_size.unwrap_or(0) };
        let size_label = if size > 0 {
            format!(" · ~{}", format_bytes(size as f64))
        } else {
            String::new()
        };
        choices.push(DownloadChoice {
            kind: ChoiceKind::Video,
            label: format!("{height}p · mp4{size_label}"),
            args: vec![
                "-f".into(),
                // Permissive fallback chain:
                //   1. video-only at exactly ${height} + best audio (ideal DASH merge)
                //   2. any progressive at exactly ${height}
                //   3. video-only at or below ${height} + best audio
                //   4. any progressive at or below ${height}
                //   5. bestvideo + bestaudio (any height)
                //   6. best progressive (any height)
                // The final `b` is the ultimate fallback — always matches.
                format!(
                    "bv*[height={height}]+ba/b[height={height}]/bv*[height<={height}]+ba/b[height<={height}]/bv*+ba/b"
                ),
                "--merge-output-format".into(),
                "mp4".into(),
            ],
        });
    }

    if choices.is_empty() {
        choices.push(DownloadChoice {
            kind: ChoiceKind::Video,
            label: "best available · mp4".into(),
            args: ["-f", "bv*+ba/b", "--merge-output-format", "mp4"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        });
    }

    let audio_size_label = audio_size
        .map(|s| format!(" · ~{}", format_bytes(s as f64)))
        .unwrap_or_default();
    choices.push(DownloadChoice {
        kind: ChoiceKind::Audio,
        label: format!("audio only · mp3{audio_size_label}"),
        args: [
            "-f", "ba/b",
            "-x",
            "--audio-format", "mp3",
            "--audio-quality", "0",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
    });

    choices
}

fn score_video(f: &RawFormat) -> f64 {
    let mut score = f.tbr.unwrap_or(0.0);
    if f.ext.as_deref() == Some("mp4") {
        score += 10_000.0;
    }
    if f.vcodec.as_deref().map_or(false, |c| c.starts_with("avc")) {
        score += 5_000.0;
    }
    score
}

const PROGRESS_PREFIX: &str = "YOINK|";

/// Spawn the download. Returns a [`DownloadGuard`] the caller can `.abort()`
/// on, and runs the yt-dlp child to completion on the current thread, driving
/// `handlers` as progress lines come in.
///
/// If `info_json_path` is set, it's passed via `--load-info-json` so yt-dlp
/// skips re-extracting metadata — the probe's work is reused.
pub fn download(
    opts: &DownloadOpts,
    handlers: &mut DownloadHandlers,
    aborted: &AtomicBool,
) -> Result<String, String> {
    let mut args: Vec<String> = Vec::new();
    if let Some(info_json) = &opts.info_json_path {
        args.push("--load-info-json".into());
        args.push(info_json.to_string_lossy().into_owned());
    } else {
        args.push(opts.url.clone());
    }
    args.extend(opts.choice.args.iter().cloned());
    args.extend([
        "--no-playlist".into(),
        "--no-warnings".into(),
        "--newline".into(),
        // --print implies --quiet, which suppresses progress bars and the
        // [Merger]/[ExtractAudio] lines we detect the processing phase from
        "--no-quiet".into(),
        "--progress".into(),
        "--progress-template".into(),
        format!("download:{PROGRESS_PREFIX}%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s"),
        "--print".into(),
        "after_move:filepath".into(),
        "--no-simulate".into(),
        "-o".into(),
        opts.out_dir.join("%(title).60s.%(ext)s").to_string_lossy().into_owned(),
    ]);
    if let Some(cookies_path) = &opts.cookies {
        args.push("--cookies".into());
        args.push(cookies_path.to_string_lossy().into_owned());
    }
    if let Some(loc) = &opts.ffmpeg_location {
        args.push("--ffmpeg-location".into());
        args.push(loc.clone());
    }

    let mut child = Command::new(&opts.ytdlp)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start yt-dlp: {e}"))?;

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;

    let mut part: u32 = 0;
    let mut total_parts: u32 = 1;
    let mut last_downloaded: u64 = 0;
    let mut filepath = String::new();
    let mut destinations: Vec<String> = Vec::new();
    let stderr_buf = Arc::new(Mutex::new(String::new()));
    let stderr_buf_clone = stderr_buf.clone();

    // drain stderr on a background thread so we can keep streaming stdout
    let stderr_handle = std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().flatten() {
            if let Ok(mut buf) = stderr_buf_clone.lock() {
                buf.push_str(&line);
                buf.push('\n');
            }
        }
    });

    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let line = line.unwrap_or_default();
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix(PROGRESS_PREFIX) {
            let mut parts = rest.split('|');
            let downloaded = parts.next().and_then(to_number).unwrap_or(0.0);
            let total = parts.next().and_then(to_number);
            let total_est = parts.next().and_then(to_number);
            let speed = parts.next().and_then(to_number);
            let eta = parts.next().and_then(to_number);
            let _ = (&mut parts,); // silence unused mut warning if any
            let downloaded_bytes = downloaded as u64;
            if downloaded_bytes < last_downloaded {
                part += 1;
            }
            last_downloaded = downloaded_bytes;
            (handlers.on_progress)(DownloadProgress {
                downloaded_bytes,
                total_bytes: total.or(total_est).map(|v| v as u64),
                speed,
                eta,
                part,
                total_parts,
            });
        } else if line.contains("Downloading 1 format(s):") {
            if let Some(after) = line.split("format(s):").nth(1) {
                total_parts = after.trim().split('+').count() as u32;
            }
        } else if line.contains("[Merger]") || line.contains("[ExtractAudio]") {
            if let Some(target) = regex_extract_target(line) {
                destinations.push(target);
            }
            (handlers.on_processing)();
        } else if let Some(dest) = line.strip_prefix("[download] Destination: ") {
            destinations.push(dest.to_string());
        } else if Path::new(line).is_absolute() {
            filepath = line.to_string();
        }

        if aborted.load(Ordering::SeqCst) {
            let _ = child.kill();
            break;
        }
    }

    let _ = stderr_handle.join();
    let status = child.wait().map_err(|e| format!("yt-dlp wait failed: {e}"))?;

    if aborted.load(Ordering::SeqCst) {
        let _ = remove_partials(&destinations);
        return Err("Download cancelled.".into());
    }

    if status.success() && !filepath.is_empty() {
        Ok(filepath)
    } else {
        let stderr_text = stderr_buf.lock().map(|s| s.clone()).unwrap_or_default();
        Err(clean_yt_dlp_error(&stderr_text)
            .unwrap_or_else(|| format!("Download failed (yt-dlp exit code {}).", status.code().unwrap_or(-1))))
    }
}

/// Public options struct for [`download`] — keeps the call site readable.
#[derive(Debug, Clone)]
pub struct DownloadOpts {
    pub ytdlp: String,
    pub ffmpeg_location: Option<String>,
    pub url: String,
    pub info_json_path: Option<PathBuf>,
    pub choice: DownloadChoice,
    pub out_dir: PathBuf,
    /// Optional path to a Netscape-format cookies file. Required by some
    /// sites (X/Twitter, Facebook) for media access.
    pub cookies: Option<PathBuf>,
}

fn wait_with_abort(
    mut child: Child,
    aborted: &AtomicBool,
) -> Result<std::process::Output, String> {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout.map(|s| {
                    let mut buf = Vec::new();
                    let _ = std::io::copy(&mut std::io::BufReader::new(s), &mut buf);
                    buf
                }).unwrap_or_default();
                let stderr = stderr.map(|s| {
                    let mut buf = Vec::new();
                    let _ = std::io::copy(&mut std::io::BufReader::new(s), &mut buf);
                    buf
                }).unwrap_or_default();
                return Ok(std::process::Output { status, stdout, stderr });
            }
            Ok(None) => {
                if aborted.load(Ordering::SeqCst) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("aborted".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("wait failed: {e}")),
        }
    }
}

fn regex_extract_target(line: &str) -> Option<String> {
    // [Merger] Merging formats into "<path>"
    if let Some(rest) = line.strip_prefix("[Merger] Merging formats into ") {
        let trimmed = rest.trim();
        if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
            return Some(trimmed[1..trimmed.len() - 1].to_string());
        }
        return Some(trimmed.to_string());
    }
    // [ExtractAudio] Destination: <path>
    if let Some(rest) = line.strip_prefix("[ExtractAudio] Destination: ") {
        return Some(rest.trim().to_string());
    }
    None
}

fn remove_partials(destinations: &[String]) -> std::io::Result<()> {
    for dest in destinations {
        for path in [dest, &format!("{dest}.part"), &format!("{dest}.ytdl")] {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

fn to_number(value: &str) -> Option<f64> {
    if value.is_empty() || value == "NA" || value == "None" {
        return None;
    }
    value.parse::<f64>().ok()
}

fn clean_yt_dlp_error(stderr: &str) -> Option<String> {
    let last = stderr
        .lines()
        .map(|l| l.trim())
        .filter(|l| l.starts_with("ERROR:"))
        .last()?;
    // strip "ERROR: " and any leading "[extractor] " tag
    let after = last.strip_prefix("ERROR:").unwrap_or(last).trim_start();
    let cleaned = if let Some(rest) = after.strip_prefix('[') {
        rest.find(']').map(|i| &rest[i + 1..]).unwrap_or(after).trim_start()
    } else {
        after
    };
    Some(enhance_error_message(cleaned))
}

/// Detect known yt-dlp error patterns and append a one-line actionable hint.
/// This is the difference between "Unsupported URL" (cryptic) and
/// "Unsupported URL — your yt-dlp may be outdated; try `zoinks --update`".
fn enhance_error_message(msg: &str) -> String {
    let lower = msg.to_lowercase();
    if lower.contains("unsupported url") {
        return format!(
            "{msg}\n  ↳ yt-dlp doesn't recognise this URL. Run `zoinks --update` to fetch the\n     latest yt-dlp release, or pass `--cookies <file>` for sites that\n     require login (X, Facebook, Instagram)."
        );
    }
    if lower.contains("no video formats found") {
        return format!(
            "{msg}\n  ↳ yt-dlp got the page but couldn't extract media. This usually means\n     the site requires login — pass `--cookies <file>` with a Netscape-\n     format cookies file exported from your browser."
        );
    }
    if lower.contains("requested format is not available") {
        return format!(
            "{msg}\n  ↳ none of the format selectors matched. Try the `best available` option,\n     or run `yt-dlp --list-formats <url>` to see what's actually offered."
        );
    }
    if lower.contains("unable to extract") || lower.contains("login required") {
        return format!(
            "{msg}\n  ↳ this site requires authentication. Pass `--cookies <file>` with a\n     Netscape-format cookies file exported from your browser."
        );
    }
    if lower.contains("http error 429") || lower.contains("too many requests") {
        return format!(
            "{msg}\n  ↳ rate-limited. Wait a minute, or pass `--cookies <file>` to authenticate."
        );
    }
    msg.to_string()
}

// some callers (probe) need a fallible error message — never used right now
// but kept around so the public API stays stable.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_yt_dlp_error_with_tag() {
        // TS regex: /^ERROR:\s*(\[[^\]]+\]\s*)?/  — strips "ERROR: " and a
        // single leading "[extractor] " tag, leaving the rest of the message.
        // The enhance_error_message layer doesn't fire here because "xxx:
        // Video unavailable" doesn't match any known pattern.
        let stderr = "WARNING: something\nERROR: [youtube] xxx: Video unavailable";
        assert_eq!(
            clean_yt_dlp_error(stderr),
            Some("xxx: Video unavailable".into())
        );
    }

    #[test]
    fn enhances_unsupported_url_error() {
        let stderr = "ERROR: [generic] Unsupported URL: https://x.com/foo/status/123";
        let result = clean_yt_dlp_error(stderr).unwrap();
        assert!(result.contains("Unsupported URL"));
        assert!(result.contains("zoinks --update"));
    }

    #[test]
    fn enhances_no_video_formats_error() {
        let stderr = "ERROR: [facebook] 123: No video formats found!; please report this issue";
        let result = clean_yt_dlp_error(stderr).unwrap();
        assert!(result.contains("No video formats"));
        assert!(result.contains("--cookies"));
    }

    #[test]
    fn returns_none_when_no_error_line() {
        assert_eq!(clean_yt_dlp_error("WARNING: nothing here"), None);
    }

    #[test]
    fn build_choices_picks_heights() {
        let info = VideoInfo {
            title: "t".into(),
            uploader: None,
            duration: None,
            webpage_url: None,
            extractor_key: None,
            formats: vec![
                RawFormat {
                    format_id: "137".into(),
                    ext: Some("mp4".into()),
                    vcodec: Some("avc1".into()),
                    acodec: Some("none".into()),
                    height: Some(1080),
                    width: None,
                    abr: None,
                    tbr: Some(4500.0),
                    filesize: Some(50_000_000),
                    filesize_approx: None,
                },
                RawFormat {
                    format_id: "22".into(),
                    ext: Some("mp4".into()),
                    vcodec: Some("avc1".into()),
                    acodec: Some("mp4a".into()),
                    height: Some(720),
                    width: None,
                    abr: None,
                    tbr: Some(1500.0),
                    filesize: Some(20_000_000),
                    filesize_approx: None,
                },
                RawFormat {
                    format_id: "251".into(),
                    ext: Some("webm".into()),
                    vcodec: Some("none".into()),
                    acodec: Some("opus".into()),
                    height: None,
                    width: None,
                    abr: Some(160.0),
                    tbr: None,
                    filesize: Some(4_000_000),
                    filesize_approx: None,
                },
            ],
        };
        let choices = build_choices(&info);
        // 1080, 720, plus audio-only mp3
        assert_eq!(choices.len(), 3);
        assert!(choices[0].label.starts_with("1080p"));
        assert!(choices[2].label.starts_with("audio only"));
    }

    #[test]
    fn extract_merger_target() {
        let line = r#"[Merger] Merging formats into "/tmp/Merged.mp4""#;
        assert_eq!(
            regex_extract_target(line),
            Some("/tmp/Merged.mp4".into())
        );
    }

    #[test]
    fn extract_audio_target() {
        let line = "[ExtractAudio] Destination: /tmp/Song.mp3";
        assert_eq!(
            regex_extract_target(line),
            Some("/tmp/Song.mp3".into())
        );
    }
}
