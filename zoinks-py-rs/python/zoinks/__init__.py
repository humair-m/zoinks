"""zoinks — zoink any video. paste. zoink. done.

A Python + Rust port of Pablo Stanley's TS `yoinks` TUI
(https://github.com/pablostanley/yoinks, MIT-licensed). All credit for the
original design, tagline, block-character logo, UX flow, and theme palette
belongs to Pablo Stanley.

The hot paths (yt-dlp orchestration, format/clipboard/history helpers) live
in a Rust core compiled via PyO3 and imported as `zoinks._zoinks_core`. This
Python side provides the textual-based TUI and a friendly API surface.

Public surface:
    from zoinks import Zoinks
    y = Zoinks()
    info = y.probe("https://youtu.be/...")
    choices = y.build_choices(info)
    y.download(info, choices[0], on_progress=lambda p: print(p))

Or the full TUI:
    python -m zoinks
    zoinks   # if installed via `pip install zoinks`
"""

from .core import (
    Zoinks,
    Platform,
    VideoInfo,
    Format,
    DownloadChoice,
    add_to_history,
    default_out_dir,
    detect_platform,
    is_probably_url,
    load_history,
    read_clipboard,
    version,
    zoinks_bin_dir,
)
from .cli import main

__all__ = [
    "Zoinks",
    "Platform",
    "VideoInfo",
    "Format",
    "DownloadChoice",
    "add_to_history",
    "default_out_dir",
    "detect_platform",
    "is_probably_url",
    "load_history",
    "main",
    "read_clipboard",
    "version",
    "zoinks_bin_dir",
]

__version__ = version()
