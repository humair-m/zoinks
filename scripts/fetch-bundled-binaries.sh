#!/usr/bin/env bash
# Fetch the yt-dlp + ffmpeg binaries that get embedded into the Rust binary
# via include_bytes!. Required only when rebuilding from source with
# `--features bundled`. Skip if you only want to develop the Python TUI.
set -e

cd "$(dirname "$0")/.."
mkdir -p bundled
cd bundled

if [ ! -f yt-dlp ]; then
  echo "Fetching yt-dlp..."
  curl -L -o yt-dlp "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux"
  chmod +x yt-dlp
fi

if [ ! -f ffmpeg ]; then
  echo "Fetching ffmpeg..."
  curl -L -o ffmpeg.tar.xz "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz"
  tar xf ffmpeg.tar.xz
  cp ffmpeg-master-latest-linux64-gpl/bin/ffmpeg .
  cp ffmpeg-master-latest-linux64-gpl/bin/ffprobe .
  rm -rf ffmpeg-master-latest-linux64-gpl ffmpeg.tar.xz
  chmod +x ffmpeg ffprobe
  # optional: compress with upx to shrink from 168 MB to 58 MB
  if command -v upx >/dev/null 2>&1; then
    echo "Compressing ffmpeg with upx..."
    upx -9 ffmpeg || true
  fi
fi

echo
echo "Done. bundled/ now contains:"
ls -lh
