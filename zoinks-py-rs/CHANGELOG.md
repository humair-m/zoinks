# Changelog

All notable changes to this Python + Rust port of `zoinks` will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.1] — 2026-10-05

### Fixed

- **YouTube "Requested format is not available"** — the format selector now
  has a 6-step fallback chain (`bv*[height=N]+ba/b[height=N]/bv*[height<=N]+ba/b[height<=N]/bv*+ba/b`)
  with the ultimate `b` always matching. Previously some YouTube URLs failed
  because none of the 4 alternatives matched the available formats.
- **X "Unsupported URL"** — added a `yt_dlp_version_is_recent` check: if the
  system yt-dlp predates 2024 (the case for Debian's `python3-yt-dlp`
  package, which silently fails on X's 19-digit snowflake IDs), zoinks now
  refuses to use it and re-downloads the latest release.
- **Facebook "No video formats found"** — added actionable error messages.
  yt-dlp errors that match known patterns (`Unsupported URL`, `No video
  formats found`, `Requested format is not available`, `login required`,
  `429 Too Many Requests`) now append a one-line hint about what to do.
- **Rust TUI missing retry-on-failure** — the Rust TUI now retries downloads
  once with a fresh probe when the cached media URLs expire (parity with the
  Python TUI, which already had this).

### Added

- **`--cookies <file>` flag** — pass a Netscape-format cookies file to
  yt-dlp. Required for sites that need login (X/Twitter, Facebook,
  Instagram). Available on both the Rust binary and the Python CLI, and as
  a `cookies=` kwarg on the `Zoinks.probe()` / `Zoinks.download()` Python API.
- **`--update` / `-U` flag** — refresh the bundled yt-dlp binary on demand.
  Also exposed as `Zoinks.update_ytdlp()` in the Python API.
- **Auto-update on `Unsupported URL`** — both the Rust TUI and the Python
  TUI now run `yt-dlp -U` and retry the probe when yt-dlp rejects a URL,
  rather than just surfacing the error.
- **`NOTICE` file** — explicit attribution of what was ported from Pablo
  Stanley's original `yoinks` and what is new in `zoinks`.

## [0.4.0] — 2026-10-05

### Added

- **Rust core crate** (`zoinks`) that implements the full `yt-dlp` wrapper, format
  helpers, platform detection, theme palette, history persistence, and clipboard
  reading. All modules are unit-tested (26 tests).
- **PyO3 bindings** (`src/py.rs`) exposing a Python-friendly `Zoinks` class plus
  `VideoInfo`, `Format`, `DownloadChoice`, `Platform` types and the top-level
  helpers `detect_platform`, `is_probably_url`, `read_clipboard`, `load_history`,
  `add_to_history`, `default_out_dir`, `zoinks_bin_dir`, `version`. Built via
  `maturin` with `abi3-py38`, so a single wheel covers CPython 3.8 through 3.13.
- **Standalone Rust TUI binary** (`zoinks-tui`) built with `ratatui` + `crossterm`.
  Same phase machine as the original Ink app: input → probing → picking →
  downloading → done / error. Theme switching via `^t`. No Python required.
- **Python textual TUI** (`python/zoinks/tui/app.py`) that mirrors the Rust
  binary's UX but is driven by the Rust core via the PyO3 bindings. Cancellable
  probe + download via the shared `AtomicBool`.
- **CLI parser** in both Rust (`src/args.rs`) and Python (`python/zoinks/args.py`)
  with identical surface: `-h` / `--help`, `-v` / `--version`, `--theme <mode>`,
  positional url. Error strings match upstream verbatim.
- **Pre-built binaries** published to the GitHub releases page:
  - `zoinks-tui-x86_64-unknown-linux-gnu`
  - `zoinks-tui-aarch64-unknown-linux-gnu`
  - `zoinks-tui-x86_64-apple-darwin`
  - `zoinks-tui-aarch64-apple-darwin`
  - `zoinks-tui-x86_64-pc-windows-gnu.exe`
  - `zoinks-<version>-py3-none-{manylinux_2_17,macos_11,win_amd64}.whl`
- **`CONTRIBUTING.md`** with build, test, and release instructions.
- **`.github/workflows/release.yml`** (planned) matrix that builds and uploads
  every binary + wheel on tag push.

### Changed

- Re-licensed the implementation as a port under the same MIT license.
  Attribution to [Pablo Stanley](https://github.com/pablostanley) is preserved
  in `README.md` and in every translated module's docstring.

### Removed

- The TS / React / Ink / `tsup` toolchain — replaced by Rust + PyO3 + textual.

## [0.3.1] — upstream

The TS original by Pablo Stanley. See
[pablostanley/yoinks](https://github.com/pablostanley/yoinks/releases) for
release notes upstream of this fork.
