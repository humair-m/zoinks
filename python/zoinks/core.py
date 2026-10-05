"""Friendly re-exports of the Rust core.

The actual Rust extension module is `zoinks._zoinks_core` (compiled by
maturin). We re-export its types/functions at the package root so users
don't have to know about the `_` prefix.
"""

from ._zoinks_core import (  # type: ignore[attr-defined]
    DownloadChoice,
    Format,
    Platform,
    VideoInfo,
    Zoinks,
    version,
)
from ._zoinks_core import (
    add_to_history_py as add_to_history,
)
from ._zoinks_core import (
    default_out_dir_py as default_out_dir,
)
from ._zoinks_core import (
    detect_platform_py as detect_platform,
)
from ._zoinks_core import (
    is_probably_url_py as is_probably_url,
)
from ._zoinks_core import (
    load_history_py as load_history,
)
from ._zoinks_core import (
    read_clipboard_py as read_clipboard,
)
from ._zoinks_core import (
    zoinks_bin_dir_py as zoinks_bin_dir,
)

__all__ = [
    "DownloadChoice",
    "Format",
    "Platform",
    "VideoInfo",
    "Zoinks",
    "add_to_history",
    "default_out_dir",
    "detect_platform",
    "is_probably_url",
    "load_history",
    "read_clipboard",
    "version",
    "zoinks_bin_dir",
]
