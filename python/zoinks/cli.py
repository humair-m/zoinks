"""CLI entry point — `zoinks` console script.

Parses args, then either:
  - prints help/version and exits
  - runs `zoinks --update` to refresh the bundled yt-dlp
  - launches the textual TUI (`ZoinksApp`) with optional `--cookies`
"""

from __future__ import annotations

import sys
from typing import Optional, Sequence

from .args import parse_args, HELP
from .tui.app import ZoinksApp
from . import version, Zoinks


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = parse_args(list(argv) if argv is not None else sys.argv[1:])

    if args.error:
        sys.stderr.write(f"zoinks: {args.error}\nTry “zoinks --help” for usage.\n")
        return 1

    if args.help:
        sys.stdout.write(HELP + "\n")
        return 0

    if args.version:
        sys.stdout.write(version() + "\n")
        return 0

    # `--update` short-circuits: ensure yt-dlp is present, then run -U.
    if args.update:
        z = Zoinks()
        try:
            z.ensure_ytdlp()
            z.update_ytdlp()
            sys.stderr.write("zoinks: yt-dlp updated.\n")
            return 0
        except Exception as e:
            sys.stderr.write(f"zoinks: update failed: {e}\n")
            return 1

    theme_mode = args.theme_mode or "auto"
    app = ZoinksApp(
        initial_url=args.initial_url,
        theme_mode=theme_mode,
        cookies=args.cookies,
    )
    try:
        filepath = app.run()
    except KeyboardInterrupt:
        return 130

    if filepath:
        sys.stdout.write(f"✓ yoinked → {filepath}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
