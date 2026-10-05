"""CLI argument parser — mirrors the Rust `src/args.rs` exactly.

Kept in Python (rather than calling into the Rust parser) so the error
strings stay localised to whichever surface the user happened to invoke.
"""

from __future__ import annotations

from dataclasses import dataclass

VALID_THEMES = ("auto", "light", "dark")


@dataclass
class CliArgs:
    help: bool = False
    version: bool = False
    update: bool = False
    initial_url: str | None = None
    theme_mode: str | None = None
    cookies: str | None = None
    cookies_from_browser: str | None = None
    error: str | None = None


def parse_args(args: list[str]) -> CliArgs:
    result = CliArgs()
    positional: list[str] = []

    i = 0
    while i < len(args):
        arg = args[i]
        if arg in ("-h", "--help"):
            result.help = True
        elif arg in ("-v", "--version"):
            result.version = True
        elif arg in ("--update", "-U"):
            result.update = True
        elif arg == "--cookies":
            i += 1
            if i >= len(args):
                return CliArgs(
                    error="--cookies needs a value: path to a Netscape-format cookies file",
                )
            result.cookies = args[i]
        elif arg.startswith("--cookies="):
            result.cookies = arg[len("--cookies="):]
        elif arg == "--cookies-from-browser":
            i += 1
            if i >= len(args):
                return CliArgs(
                    error="--cookies-from-browser needs a value: chrome, firefox, safari, edge, opera, chromium, brave, vivaldi, or whale",
                )
            result.cookies_from_browser = args[i]
        elif arg.startswith("--cookies-from-browser="):
            result.cookies_from_browser = arg[len("--cookies-from-browser="):]
        elif arg == "--theme":
            i += 1
            if i >= len(args):
                return CliArgs(
                    error="--theme needs a value: auto, light, or dark",
                )
            value = args[i]
            if value not in VALID_THEMES:
                return CliArgs(
                    error=f'unknown theme “{value}” — use auto, light, or dark',
                )
            result.theme_mode = value
        elif arg.startswith("--theme="):
            value = arg[len("--theme="):]
            if value not in VALID_THEMES:
                return CliArgs(
                    error=f'unknown theme “{value}” — use auto, light, or dark',
                )
            result.theme_mode = value
        elif arg.startswith("-"):
            return CliArgs(error=f'unknown option “{arg}”')
        else:
            positional.append(arg)
        i += 1

    if len(positional) > 1:
        return CliArgs(error="expected a single url")
    if positional:
        result.initial_url = positional[0]
    return result


HELP = """\
  zoinks — zoink any video. paste. zoink. done.

  Usage
    $ zoinks [url]

  Examples
    $ zoinks https://youtu.be/dQw4w9WgXcQ
    $ zoinks https://x.com/user/status/123456
    $ zoinks                                   (prompts for a url)
    $ zoinks --cookies-from-browser chrome <url>  (auto-pull cookies from Chrome)
    $ zoinks --cookies cookies.txt <url>           (Netscape-format cookies file)
    $ zoinks --update                             (update bundled yt-dlp)

  Options
    --theme <mode>            use auto, light, or dark for this run
    --cookies-from-browser <b>  pass cookies from chrome/firefox/safari/edge/
                              opera/chromium/brave/vivaldi/whale to yt-dlp
    --cookies <path>         pass a Netscape-format cookies file to yt-dlp
    -U, --update             update the bundled yt-dlp to the latest release
    -h, --help               show this help
    -v, --version            show version

  Downloads are saved to ~/Downloads.
  Powered by yt-dlp — YouTube, X, Instagram, Threads, TikTok & 1800+ sites.

  A port of Pablo Stanley's `yoinks` (https://github.com/pablostanley/yoinks).
"""
