//! Build script — auto-downloads yt-dlp + ffmpeg to `bundled/` if missing.
//!
//! When the `bundled` feature is on, `src/embedded.rs` does:
//!   include_bytes!("../bundled/yt-dlp")
//!   include_bytes!("../bundled/ffmpeg")
//!
//! These files are .gitignored (95 MB total). On a clean checkout (e.g., CI),
//! this build.rs fetches them automatically before cargo compiles, so
//! `cargo build --features bundled` Just Works.
//!
//! To skip the download (e.g., in CI without network), set `ZOINKS_SKIP_BUNDLED_DOWNLOAD=1`
//! and the build will fail with a clear error pointing at scripts/fetch-bundled-binaries.sh.

use std::env;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

const YT_DLP_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux";
const FFMPEG_URL: &str =
    "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz";

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
        if env::var("ZOINKS_SKIP_BUNDLED_DOWNLOAD").is_ok() {
            panic!(
                "bundled/ffmpeg is missing and ZOINKS_SKIP_BUNDLED_DOWNLOAD is set.\n\
                 Run `scripts/fetch-bundled-binaries.sh` to download it manually."
            );
        }
        download_ffmpeg(&ffmpeg_path);
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
    eprintln!("build.rs: downloading yt-dlp to {}...", dest.display());
    let rc = Command::new("curl")
        .args(["-sL", "-o"])
        .arg(dest)
        .arg(YT_DLP_URL)
        .status()
        .expect("curl is required to fetch yt-dlp");
    if !rc.success() {
        panic!(
            "build.rs: failed to download yt-dlp from {}\n\
             curl exited with {:?}\n\
             Set ZOINKS_SKIP_BUNDLED_DOWNLOAD=1 and place the file manually if your\n\
             network can't reach GitHub.",
            YT_DLP_URL,
            rc.code()
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(dest).unwrap().permissions();
        perms.set_mode(0o755);
        let _ = fs::set_permissions(dest, perms);
    }
    eprintln!("build.rs: yt-dlp downloaded");
}

fn download_ffmpeg(dest: &std::path::Path) {
    eprintln!("build.rs: downloading ffmpeg to {}...", dest.display());
    let tmp_xz = dest.with_extension("tar.xz");
    let rc = Command::new("curl")
        .args(["-sL", "-o"])
        .arg(&tmp_xz)
        .arg(FFMPEG_URL)
        .status()
        .expect("curl is required to fetch ffmpeg");
    if !rc.success() {
        panic!(
            "build.rs: failed to download ffmpeg from {}\n\
             curl exited with {:?}",
            FFMPEG_URL,
            rc.code()
        );
    }
    // verify it's actually an xz file (not a 404 HTML page)
    let mut header = [0u8; 6];
    if let Ok(mut f) = fs::File::open(&tmp_xz) {
        let _ = f.read_exact(&mut header);
    }
    // XZ magic: 0xFD '7zXZ'
    if header[0] != 0xFD || &header[1..4] != b"7zX" {
        let _ = fs::remove_file(&tmp_xz);
        panic!(
            "build.rs: downloaded ffmpeg.tar.xz is not a valid XZ archive (got header {:?}).\n\
             The release URL may have moved — check {}",
            header,
            FFMPEG_URL
        );
    }
    let rc = Command::new("tar")
        .args(["xf"])
        .arg(&tmp_xz)
        .current_dir(dest.parent().unwrap())
        .status()
        .expect("tar is required to extract ffmpeg");
    if !rc.success() {
        panic!("build.rs: failed to extract ffmpeg.tar.xz");
    }
    // The tarball extracts to ffmpeg-master-latest-linux64-gpl/bin/ffmpeg
    let extracted = dest.parent().unwrap().join("ffmpeg-master-latest-linux64-gpl/bin/ffmpeg");
    fs::rename(&extracted, dest).expect("failed to move ffmpeg into place");
    // Clean up
    let _ = fs::remove_file(&tmp_xz);
    let _ = fs::remove_dir_all(dest.parent().unwrap().join("ffmpeg-master-latest-linux64-gpl"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(dest).unwrap().permissions();
        perms.set_mode(0o755);
        let _ = fs::set_permissions(dest, perms);
    }
    eprintln!("build.rs: ffmpeg downloaded");
}
