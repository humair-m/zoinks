# Installing zoinks

## Quick start

### Option A — Python wheel (recommended)

```sh
pip install zoinks            # from PyPI (once published)
# or, from the local wheel:
pip install dist/zoinks-0.4.1-cp38-abi3-manylinux_2_39_x86_64.whl

zoinks                                  # prompts for a url
zoinks https://youtu.be/dQw4w9WgXcQ     # straight to the format picker
zoinks --theme dark                     # force the dark palette
python -m zoinks                        # equivalent
```

### Option B — Standalone Rust binary (no Python required)

```sh
# from the prebuilt binary in dist/
chmod +x dist/zoinks-tui-x86_64-unknown-linux-gnu
./dist/zoinks-tui-x86_64-unknown-linux-gnu --help
./dist/zoinks-tui-x86_64-unknown-linux-gnu https://youtu.be/dQw4w9WgXcQ
```

### Option C — Build from source

```sh
# Rust binary
cargo build --release --no-default-features --bin zoinks-tui
./target/release/zoinks-tui

# Python wheel
pip install maturin textual
maturin build --release --out dist
pip install --force-reinstall dist/zoinks-*.whl
```

## Requirements

- **Python 3.8+** for the Python package (single `abi3` wheel covers all CPython 3.8-3.13).
- **Rust 1.74+** only if building from source.
- **yt-dlp** — auto-fetched to `~/.zoinks/bin` on first run; system PATH copy used if present.
- **ffmpeg** — for merging high-res streams / mp3 extraction. Audio-only works without it.

## Uploading to your GitHub (https://github.com/humair-m)

```sh
# 1. Clone the source
git clone https://github.com/humair-m/zoinks.git
cd zoinks

# 2. Or — push from this directory:
cd /home/z/my-project/download/zoinks-py-rs
git init -b main
git remote add origin git@github.com:humair-m/zoinks.git
git add -A
git commit -m "Initial commit: zoinks Python+Rust port (v0.4.1)"
git push -u origin main

# 3. Tag the release — CI builds and uploads the binaries + wheels
git tag v0.4.1
git push origin v0.4.1
# Wait for the GitHub Actions "release" workflow to finish, then download
# the artifacts from the Releases tab.
```

## Files in `dist/`

| File | What |
|------|------|
| `zoinks-0.4.1-cp38-abi3-manylinux_2_39_x86_64.whl` | Python wheel — installs `zoinks` console script + Python package with the Rust core baked in. |
| `zoinks-tui-x86_64-unknown-linux-gnu` | Standalone Linux x86_64 Rust binary (2.5 MB, stripped). No Python needed. |

Both files were built and smoke-tested in this session.

## Source layout

See [README.md](README.md#project-layout) for the full tree.

## License

MIT — same as [upstream](https://github.com/pablostanley/yoinks).
