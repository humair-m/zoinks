//! Build script — auto-downloads yt-dlp + ffmpeg to `bundled/` if missing.
//!
//! When the `bundled` feature is on, `src/embedded.rs` does:
//!   include_bytes!("../bundled/yt-dlp")
//!   include_bytes!("../bundled/ffmpeg")
//!
//! These files are .gitignored (~95 MB total). On a clean checkout (e.g., CI),
//! this build.rs fetches them automatically before cargo compiles, so
//! `cargo build --features bundled` Just Works.
//!
//! The downloads are platform-aware:
//!   - Linux x86_64   → yt-dlp_linux              + ffmpeg-linux64-gpl
//!   - Linux aarch64  → yt-dlp_linux_aarch64      + ffmpeg-linuxarm64-gpl
//!   - macOS x86_64   → yt-dlp_macos              + ffmpeg-macos64-gpl
//!   - macOS arm64    → yt-dlp_macos              + ffmpeg-macosarm64-gpl
//!   - Windows x86_64 → yt-dlp.exe                + ffmpeg-win64-gpl
//!
//! To skip the download (e.g., in CI without network), set `ZOINKS_SKIP_BUNDLED_DOWNLOAD=1`
//! and the build will fail with a clear error pointing at scripts/fetch-bundled-binaries.sh.

use std::env;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

fn yt_dlp_asset() -> &'static str {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => "yt-dlp_linux",
        ("linux", "aarch64") => "yt-dlp_linux_aarch64",
        ("macos", _) => "yt-dlp_macos",
        ("windows", _) => "yt-dlp.exe",
        _ => "yt-dlp_linux",
    }
}

fn ffmpeg_url() -> Option<String> {
    // yt-dlp/FFmpeg-Builds only ships Linux + Windows binaries — no macOS builds.
    // For macOS we fall back to the system Homebrew ffmpeg (or any ffmpeg on PATH)
    // at runtime, so we don't embed anything at build time.
    let arch = match env::consts::ARCH {
        "x86_64" => "64",
        "aarch64" => "arm64",
        _ => "64",
    };
    match env::consts::OS {
        "linux" => Some(format!(
            "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux{}-gpl.tar.xz",
            arch
        )),
        "windows" => Some(format!(
            "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip"
        )),
        _ => None, // macOS: no static build available — fall back to system ffmpeg
    }
}

fn main() {
    // Re-run if the feature is on or the bundled files change
    println!("cargo:rerun-if-changed=src/embedded.rs");
    println!("cargo:rerun-if-changed=bundled/yt-dlp");
    println!("cargo:rerun-if-changed=bundled/ffmpeg");
    println!("cargo:rerun-if-env-changed=ZOINKS_SKIP_BUNDLED_DOWNLOAD");

    // Only fetch when the `bundled` feature is on
    if !cfg!(feature = "bundled") {
        return;
    }

    // Skip on docs.rs and other non-build contexts
    if env::var("DOCS_RS").is_ok() {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let bundled_dir = manifest_dir.join("bundled");
    let _ = fs::create_dir_all(&bundled_dir);

    let ytdlp_path = bundled_dir.join("yt-dlp");
    let ffmpeg_path = bundled_dir.join("ffmpeg");

    if !ytdlp_path.exists() {
        if env::var("ZOINKS_SKIP_BUNDLED_DOWNLOAD").is_ok() {
            panic!(
                "bundled/yt-dlp is missing and ZOINKS_SKIP_BUNDLED_DOWNLOAD is set.\n\
                 Run `scripts/fetch-bundled-binaries.sh` to download it manually, or\n\
                 unset the env var to let build.rs fetch it automatically."
            );
        }
        download_yt_dlp(&ytdlp_path);
    }

    if !ffmpeg_path.exists() {
        match ffmpeg_url() {
            Some(url) => {
                if env::var("ZOINKS_SKIP_BUNDLED_DOWNLOAD").is_ok() {
                    panic!(
                        "bundled/ffmpeg is missing and ZOINKS_SKIP_BUNDLED_DOWNLOAD is set.\n\
                         Run `scripts/fetch-bundled-binaries.sh` to download it manually."
                    );
                }
                download_ffmpeg(&ffmpeg_path, &url);
            }
            None => {
                // macOS: no static ffmpeg build available from yt-dlp/FFmpeg-Builds.
                // Don't embed anything — the runtime will fall back to system ffmpeg
                // (Homebrew: `brew install ffmpeg`).
                println!(
                    "cargo:warning=bundled/ffmpeg not embedded on this platform (macOS has no \
                     static build). The zoinks binary will use whatever ffmpeg is on PATH at \
                     runtime. Install via `brew install ffmpeg` for full functionality."
                );
                // Create an empty placeholder so embedded.rs doesn't fail to compile.
                // The runtime checks file size before extracting — 0 bytes means "skip".
                let _ = fs::write(&ffmpeg_path, b"");
            }
        }
    }

    // Sanity-check: files exist and are non-empty
    for (name, path) in [("yt-dlp", &ytdlp_path), ("ffmpeg", &ffmpeg_path)] {
        if let Ok(meta) = fs::metadata(&path) {
            if meta.len() < 1_000_000 {
                println!(
                    "cargo:warning=bundled/{} is only {} bytes — looks truncated. \
                     Delete it and re-run the build.",
                    name,
                    meta.len()
                );
            }
        }
    }

    // Compress ffmpeg with upx if available (cuts from 168 MB → 58 MB, which
    // means the embedded bytes are smaller and compilation uses less RAM).
    // yt-dlp ships already UPX-compressed.
    if let Ok(meta) = fs::metadata(&ffmpeg_path) {
        // Heuristic: < 100 MB = already compressed; >= 100 MB = needs upx
        if meta.len() >= 100_000_000 {
            if let Ok(upx) = which_upx() {
                println!("build.rs: compressing ffmpeg with upx (this takes ~30s)...");
                let rc = Command::new(&upx)
                    .args(["-9", "--lzma"])
                    .arg(&ffmpeg_path)
                    .status();
                if rc.map(|s| s.success()).unwrap_or(false) {
                    println!("build.rs: ffmpeg compressed");
                } else {
                    println!("cargo:warning=upx failed on bundled/ffmpeg — keeping the uncompressed version");
                }
            } else {
                println!(
                    "cargo:warning=bundled/ffmpeg is {} bytes (uncompressed). Install `upx` to compress it (~3x smaller binary).",
                    meta.len()
                );
            }
        }
    }
}

/// Find `upx` on PATH. Returns the path if found.
fn which_upx() -> Result<std::path::PathBuf, ()> {
    let path = env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        let candidate = std::path::Path::new(dir).join("upx");
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(())
}

fn download_yt_dlp(dest: &std::path::Path) {
    let asset = yt_dlp_asset();
    let url = format!(
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/{}",
        asset
    );
    eprintln!("build.rs: downloading yt-dlp from {} to {}...", url, dest.display());
    let rc = Command::new("curl")
        .args(["-sL", "-o"])
        .arg(dest)
        .arg(&url)
        .status()
        .expect("curl is required to fetch yt-dlp");
    if !rc.success() {
        panic!(
            "build.rs: failed to download yt-dlp from {}\n\
             curl exited with {:?}\n\
             Set ZOINKS_SKIP_BUNDLED_DOWNLOAD=1 and place the file manually if your\n\
             network can't reach GitHub.",
            url,
            rc.code()
        );
    }
    // Verify the download is non-trivial (not a 404 HTML page)
    if let Ok(meta) = fs::metadata(dest) {
        if meta.len() < 1_000_000 {
            let _ = fs::remove_file(dest);
            panic!(
                "build.rs: yt-dlp download from {} is only {} bytes — likely a 404 page.\n\
                 The asset name '{}' may be wrong for this platform.",
                url,
                meta.len(),
                asset
            );
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(dest).unwrap().permissions();
        perms.set_mode(0o755);
        let _ = fs::set_permissions(dest, perms);
    }
    eprintln!("build.rs: yt-dlp downloaded ({:?})", asset);
}

fn download_ffmpeg(dest: &std::path::Path, url: &str) {
    eprintln!("build.rs: downloading ffmpeg from {} to {}...", url, dest.display());

    // Windows ships as .zip, Linux as .tar.xz
    let is_zip = url.ends_with(".zip");
    let tmp_archive = if is_zip {
        dest.with_extension("zip")
    } else {
        dest.with_extension("tar.xz")
    };

    let rc = Command::new("curl")
        .args(["-sL", "-o"])
        .arg(&tmp_archive)
        .arg(url)
        .status()
        .expect("curl is required to fetch ffmpeg");
    if !rc.success() {
        panic!(
            "build.rs: failed to download ffmpeg from {}\n\
             curl exited with {:?}",
            url,
            rc.code()
        );
    }

    if is_zip {
        // Windows: extract via unzip or PowerShell
        let rc = Command::new("unzip")
            .args(["-q", "-o"])
            .arg(&tmp_archive)
            .arg("-d")
            .arg(dest.parent().unwrap())
            .status();
        let ok = rc.map(|s| s.success()).unwrap_or(false);
        if !ok {
            // Fall back to PowerShell's Expand-Archive
            let ps_rc = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    &format!(
                        "Expand-Archive -Force -Path '{}' -DestinationPath '{}'",
                        tmp_archive.display(),
                        dest.parent().unwrap().display()
                    ),
                ])
                .status();
            if !ps_rc.map(|s| s.success()).unwrap_or(false) {
                panic!(
                    "build.rs: failed to extract ffmpeg zip (tried unzip and PowerShell)"
                );
            }
        }
    } else {
        // Linux: tar.xz
        // verify it's actually an xz file (not a 404 HTML page)
        let mut header = [0u8; 6];
        if let Ok(mut f) = fs::File::open(&tmp_archive) {
            let _ = f.read_exact(&mut header);
        }
        // XZ magic: 0xFD '7zXZ'
        if header[0] != 0xFD || &header[1..4] != b"7zX" {
            let _ = fs::remove_file(&tmp_archive);
            panic!(
                "build.rs: downloaded ffmpeg archive is not a valid XZ file (got header {:?}).\n\
                 The release URL may have moved — check {}",
                header,
                url
            );
        }
        let rc = Command::new("tar")
            .args(["xf"])
            .arg(&tmp_archive)
            .current_dir(dest.parent().unwrap())
            .status()
            .expect("tar is required to extract ffmpeg");
        if !rc.success() {
            panic!("build.rs: failed to extract ffmpeg.tar.xz");
        }
    }

    // The archive extracts to ffmpeg-master-latest-<platform>-gpl/bin/ffmpeg
    // (or ffmpeg.exe on Windows). Find it via glob.
    let parent = dest.parent().unwrap();
    let mut extracted: Option<PathBuf> = None;
    let ffmpeg_name = if cfg!(target_os = "windows") { "ffmpeg.exe" } else { "ffmpeg" };
    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Match ffmpeg-master-latest-* directories
            if name_str.starts_with("ffmpeg-master-latest-") || name_str.starts_with("ffmpeg-master-") {
                let candidate = entry.path().join("bin").join(ffmpeg_name);
                if candidate.exists() {
                    extracted = Some(candidate);
                    break;
                }
                // Some builds have a different layout
                let candidate2 = entry.path().join("bin").join(ffmpeg_name);
                if candidate2.exists() {
                    extracted = Some(candidate2);
                    break;
                }
            }
        }
    }
    let extracted = extracted.unwrap_or_else(|| {
        panic!(
            "build.rs: couldn't find {} inside the extracted archive — \
             check that the URL returned a valid ffmpeg-master-latest-* archive",
            ffmpeg_name
        )
    });
    fs::rename(&extracted, dest).expect("failed to move ffmpeg into place");

    // Clean up the archive and extracted directory
    let _ = fs::remove_file(&tmp_archive);
    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("ffmpeg-master-latest-") || name_str.starts_with("ffmpeg-master-") {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(dest).unwrap().permissions();
        perms.set_mode(0o755);
        let _ = fs::set_permissions(dest, perms);
    }
    eprintln!("build.rs: ffmpeg downloaded");
}
