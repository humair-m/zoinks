"""Textual TUI — same phase machine as the original TS Ink app.

Phases: input → probing → picking → downloading → done / error.
Cancellation is wired through the Rust core's `aborted` AtomicBool,
shared across the background worker thread the textual app spawns.
"""

from __future__ import annotations

import typing
from dataclasses import dataclass, field

from textual import work
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Center, Vertical
from textual.widgets import (
    Input,
    Label,
    ListItem,
    ListView,
    Static,
)

from ..core import (
    Zoinks,
    add_to_history,
    is_probably_url,
    load_history,
    read_clipboard,
)

TAGLINE = "zoink any video. paste. zoink. done."
SUBTITLE = "youtube · x · instagram · threads · tiktok · +1800 more"
YOINK_BUTTON = "zoink"
DONE_LABEL = "↵ zoink another"

LOGO = """\
███ █▀█ ▀█▀ █▀▄█ █ █ █▀▀
 ▄▀ █ ▓  ▓  █  ▓ ▓▀▄ ▀▀▓
▀▀▀ ▀▀▀ ▀▀▀ ▀  ▀ ▀ ▀ ▀▀▀
"""


def _format_bytes(n: float) -> str:
    if not isinstance(n, (int, float)) or n <= 0:
        return ""
    units = ["B", "KB", "MB", "GB"]
    v, u = float(n), 0
    while v >= 1024 and u < len(units) - 1:
        v /= 1024
        u += 1
    return f"{round(v) if v >= 10 or u == 0 else round(v, 1)} {units[u]}"


def _format_duration(seconds: float) -> str:
    if not isinstance(seconds, (int, float)) or seconds <= 0:
        return ""
    s = round(seconds)
    h, rem = divmod(s, 3600)
    m, sec = divmod(rem, 60)
    if h > 0:
        return f"{h}:{m:02}:{sec:02}"
    return f"{m}:{sec:02}"


def _truncate(text: str, max_len: int) -> str:
    if len(text) <= max_len:
        return text
    return text[: max_len - 1] + "…"


@dataclass
class Phase:
    name: str = "input"
    warning: str | None = None
    status: str | None = None
    info: object | None = None
    choices: list[object] = field(default_factory=list)
    progress: dict | None = None
    processing: bool = False
    refreshing: bool = False
    filepath: str | None = None
    message: str | None = None


class ZoinksApp(App):
    """Textual app for zoinks — paste, zoink, done."""

    CSS = """
    Screen {
        align: center middle;
        background: $surface;
    }
    #logo { text-align: center; color: $primary; }
    #tagline { text-align: center; color: $primary; }
    #subtitle { text-align: center; color: $text-muted; }
    #input-wrap { width: 70; height: auto; padding: 1 2; border: round $primary; }
    #input-title { color: $primary; text-style: bold; }
    Input { width: 100%; }
    #yoink-button { text-align: center; color: $text; background: $primary; padding: 0 2; }
    #status { color: $text-muted; text-align: center; }
    #picker { width: 100%; height: auto; }
    ListView { width: 60; height: auto; max-height: 12; border: round $primary; }
    ListItem { color: $text; }
    ListItem.--highlight { text-style: bold; color: $primary; }
    #progress-bar { width: 30; }
    #footer { dock: bottom; height: 1; text-align: center; color: $text-muted; }
    """

    BINDINGS: typing.ClassVar[list[Binding]] = [
        Binding("ctrl+t", "cycle_theme", "theme", show=True),
        Binding("ctrl+c", "quit", "quit", show=True),
    ]

    def __init__(
        self,
        initial_url: str | None = None,
        theme_mode: str = "auto",
        cookies: str | None = None,
        cookies_from_browser: str | None = None,
    ) -> None:
        super().__init__()
        self.initial_url = initial_url
        self.theme_mode = theme_mode
        self.cookies = cookies
        self.cookies_from_browser = cookies_from_browser
        self.core = Zoinks()
        self.phase = Phase(name="input")
        self.url_input = ""
        self.url = ""
        self.clipboard_url = self._detect_clipboard()
        self.history = load_history()
        self.last_filepath: str | None = None
        self.list_view: ListView | None = None

    # ---- lifecycle ------------------------------------------------------

    def _detect_clipboard(self) -> str | None:
        if self.initial_url:
            return None
        try:
            clipped = read_clipboard().strip()
        except Exception:  # noqa: BLE001
            return None
        if clipped and not any(c.isspace() for c in clipped) and is_probably_url(clipped):
            return clipped
        return None

    def on_mount(self) -> None:
        if self.initial_url:
            self.start_probe(self.initial_url)
        else:
            self.phase = Phase(name="input")

    def compose(self) -> ComposeResult:
        with Center(), Vertical():
            yield Static(LOGO, id="logo")
            yield Static(TAGLINE, id="tagline")
            yield Static(SUBTITLE, id="subtitle")
            yield from self._compose_main()
            yield Static(self._footer_text(), id="footer")

    def _compose_main(self) -> ComposeResult:
        if self.phase.name == "input":
            yield from self._compose_input()
        elif self.phase.name == "probing":
            yield from self._compose_probing()
        elif self.phase.name == "picking":
            yield from self._compose_picking()
        elif self.phase.name == "downloading":
            yield from self._compose_downloading()
        elif self.phase.name == "done":
            yield from self._compose_done()
        elif self.phase.name == "error":
            yield from self._compose_error()

    def _compose_input(self) -> ComposeResult:
        with Vertical(id="input-wrap"):
            yield Static("Paste a link", id="input-title")
            yield Input(
                value=self.url_input,
                placeholder="https://youtube.com/watch?v=…",
                id="url-input",
            )
            yield Static(f"  {YOINK_BUTTON}  ", id="yoink-button")
            hint = self._input_hint()
            if hint:
                yield Static(hint, id="status")

    def _compose_probing(self) -> ComposeResult:
        with Vertical(id="input-wrap"):
            yield Static("zoink", id="input-title")
            yield Static(_truncate(self.url, 60), id="status")
            yield Static(f"⠋ {self.phase.status or 'warming up…'}", id="status")

    def _compose_picking(self) -> ComposeResult:
        info = self.phase.info
        title = getattr(info, "title", "") if info else ""
        platform_label = ""
        if self.phase.info and hasattr(self.phase.info, "extractor_key"):
            platform_label = getattr(self.phase.info, "extractor_key", "") or ""
        meta = " · ".join(filter(None, [
            platform_label,
            _format_duration(getattr(info, "duration", 0) or 0) if info else "",
            getattr(info, "uploader", "") if info else "",
        ]))
        with Vertical(id="picker"):
            yield Static(_truncate(title, 60), id="input-title")
            yield Static(meta, id="status")
            items = [
                ListItem(Label(self._choice_label(c))) for c in self.phase.choices
            ]
            lv = ListView(*items, id="choice-list")
            self.list_view = lv
            yield lv

    def _compose_downloading(self) -> ComposeResult:
        info = self.phase.info
        title = getattr(info, "title", "") if info else ""
        choice_label = ""
        progress = self.phase.progress or {}
        bar_pct = 0
        if progress.get("total_bytes"):
            bar_pct = int(100 * progress["downloaded_bytes"] / progress["total_bytes"])
        bar = "█" * (bar_pct // 4) + "░" * (25 - bar_pct // 4)
        lines = [f"{_truncate(title, 42)} · {choice_label}", "", f"[{bar}] {bar_pct:>3}%"]
        if self.phase.processing:
            lines.append("⠋ processing…")
        elif progress:
            speed = progress.get("speed", 0) or 0
            eta = progress.get("eta", 0) or 0
            lines.append(
                f"{_format_bytes(speed) + '/s' if speed else '':>10}  "
                f"{_format_duration(eta) + ' left' if eta else '':<12}"
            )
        else:
            msg = " link expired — grabbing a fresh one…" if self.phase.refreshing else " starting download…"
            lines.append(f"⠋{msg}")
        with Vertical(id="picker"):
            yield Static("\n".join(lines), id="status")

    def _compose_done(self) -> ComposeResult:
        with Vertical(id="picker"):
            yield Static(f"✓ yoinked!\nfind your file in:\n{self.phase.filepath}", id="status")
            yield Static(f"  {DONE_LABEL}  ", id="yoink-button")

    def _compose_error(self) -> ComposeResult:
        with Vertical(id="picker"):
            yield Static(f"✗ {self.phase.message}", id="status")

    # ---- helpers --------------------------------------------------------

    def _input_hint(self) -> str:
        if self.phase.warning:
            return f"✗ {self.phase.warning}"
        if self.clipboard_url and not self.url_input:
            return "link in your clipboard — ⇥ to paste it"
        return ""

    def _footer_text(self) -> str:
        hints = []
        name = self.phase.name
        if name == "input":
            hints = [("↵", "zoink"), ("^c", "quit")]
            if self.history:
                hints.insert(1, ("↑", "history"))
        elif name == "probing":
            hints = [("esc", "cancel"), ("^c", "quit")]
        elif name == "picking":
            hints = [("↑↓", "choose"), ("↵", "zoink"), ("esc", "back"), ("^c", "quit")]
        elif name == "downloading":
            hints = [("esc", "cancel"), ("^c", "quit")]
        elif name == "done":
            hints = [("↵", "zoink another"), ("^c", "quit")]
        elif name == "error":
            hints = [("↵", "try again"), ("^c", "quit")]
        hints.append(("^t", f"theme:{self.theme_mode}"))
        return "  ".join(f"{k} {v}" for k, v in hints)

    def _choice_label(self, choice) -> str:
        prefix = "♪ " if choice.kind == "audio" else "▶ "
        return f"{prefix}{choice.label}"

    # ---- actions --------------------------------------------------------

    def action_cycle_theme(self) -> None:
        order = ["auto", "dark", "light"]
        idx = order.index(self.theme_mode) if self.theme_mode in order else 0
        self.theme_mode = order[(idx + 1) % len(order)]
        self._refresh_footer()

    def _refresh_footer(self) -> None:
        try:
            footer = self.query_one("#footer", Static)
            footer.update(self._footer_text())
        except Exception:  # noqa: BLE001, S110
            pass

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id == "url-input":
            self.url_input = event.value

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "url-input":
            self._submit_url(event.value)

    def on_key(self, event) -> None:
        if event.key == "escape":
            if self.phase.name in ("probing", "downloading"):
                self._cancel_run()
            elif self.phase.name in ("picking", "error", "done"):
                self._reset_to_input()
        elif event.key == "enter" and self.phase.name in ("error", "done"):
            self._reset_to_input()

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        idx = event.list_view.index or 0
        self._start_download(idx)

    # ---- phase transitions ---------------------------------------------

    def _submit_url(self, value: str) -> None:
        trimmed = value.strip()
        if not is_probably_url(trimmed):
            self.phase = Phase(name="input", warning="that doesn't look like a link — paste a full url")
            self._rerender()
            return
        self.url = trimmed
        self.start_probe(trimmed)

    def _reset_to_input(self) -> None:
        self.url = ""
        self.url_input = ""
        self.phase = Phase(name="input")
        self._rerender()

    def _cancel_run(self) -> None:
        self.core.abort()
        kept_url = self.url
        self._reset_to_input()
        self.url_input = kept_url

    def start_probe(self, url: str) -> None:
        self.core.reset_abort()
        self.url = url
        self.phase = Phase(name="probing", status="warming up…")
        self._rerender()
        self._probe_worker(url)

    @work(exclusive=True, exit_on_error=False, name="zoinks-probe")
    async def _probe_worker(self, url: str) -> None:
        try:
            self.phase = Phase(name="probing", status="ensuring yt-dlp…")
            self._rerender()
            self.core.ensure_ytdlp(lambda msg: self._set_probe_status(msg))
            self.phase = Phase(name="probing", status="fetching video info…")
            self._rerender()
            info = self.core.probe(url, cookies=self.cookies, cookies_from_browser=self.cookies_from_browser)
            choices = self.core.build_choices(info)
            self.phase = Phase(name="picking", info=info, choices=choices)
            self._rerender()
        except Exception as e:  # noqa: BLE001
            if "aborted" in str(e).lower():
                return
            # If yt-dlp rejected the URL, try updating yt-dlp and retry once.
            # Common case: the bundled yt-dlp is a few weeks old and the
            # site shipped a new URL scheme (X snowflake IDs etc.).
            msg = str(e)
            if "Unsupported URL" in msg or "unable to extract" in msg:
                self.phase = Phase(
                    name="probing",
                    status="yt-dlp rejected the URL — updating and retrying…",
                )
                self._rerender()
                try:
                    self.core.update_ytdlp()
                    info = self.core.probe(url, cookies=self.cookies, cookies_from_browser=self.cookies_from_browser)
                    choices = self.core.build_choices(info)
                    self.phase = Phase(name="picking", info=info, choices=choices)
                    self._rerender()
                    return
                except Exception as e2:  # noqa: BLE001
                    if "aborted" in str(e2).lower():
                        return
                    self.phase = Phase(name="error", message=str(e2))
                    self._rerender()
                    return
            self.phase = Phase(name="error", message=msg)
            self._rerender()

    def _set_probe_status(self, msg: str) -> None:
        if self.phase.name == "probing":
            self.phase.status = msg
            self._rerender()

    def _start_download(self, idx: int) -> None:
        if idx >= len(self.phase.choices):
            return
        choice = self.phase.choices[idx]
        self.core.reset_abort()
        self.phase = Phase(
            name="downloading",
            info=self.phase.info,
            progress=None,
            processing=False,
            refreshing=False,
        )
        self._rerender()
        self._download_worker(self.url, choice, getattr(self.phase.info, "info_json_path", None))

    @work(exclusive=True, exit_on_error=False, name="zoinks-download")
    async def _download_worker(self, url: str, choice, info_json_path: str | None) -> None:
        info_json = info_json_path
        try:
            try:
                filepath = self.core.download(
                    url,
                    choice,
                    info_json_path=info_json,
                    cookies=self.cookies,
                    cookies_from_browser=self.cookies_from_browser,
                    on_progress=self._on_progress,
                    on_processing=self._on_processing,
                )
            except Exception:  # noqa: BLE001
                # media URLs in the cached info can expire — retry with fresh extraction
                self.phase.refreshing = True
                self.phase.progress = None
                self._rerender()
                filepath = self.core.download(
                    url,
                    choice,
                    info_json_path=None,
                    cookies=self.cookies,
                    cookies_from_browser=self.cookies_from_browser,
                    on_progress=self._on_progress,
                    on_processing=self._on_processing,
                )
            self.last_filepath = filepath
            self.history = add_to_history(url)
            self.phase = Phase(name="done", filepath=filepath)
            self._rerender()
        except Exception as e:  # noqa: BLE001
            if "cancelled" in str(e).lower() or "aborted" in str(e).lower():
                return
            self.phase = Phase(name="error", message=str(e))
            self._rerender()

    def _on_progress(self, p: dict) -> None:
        if self.phase.name != "downloading":
            return
        self.phase.progress = p
        self.phase.processing = False
        self._rerender()

    def _on_processing(self) -> None:
        if self.phase.name != "downloading":
            return
        self.phase.processing = True
        self._rerender()

    # ---- rerender -------------------------------------------------------

    def _rerender(self) -> None:
        """Rebuild the main widget tree to reflect the current phase."""
        # textual doesn't have a one-shot "recompose" we can call mid-keypress
        # without async, so we walk the children we know about and update them.
        # This is intentionally simple — the original Ink app re-rendered the
        # whole tree on every state change, and textual's reactive model
        # supports the same pattern.
        try:
            # remove existing main container children and re-compose
            for child_id in ["input-wrap", "picker"]:
                try:
                    widget = self.query_one(f"#{child_id}")
                    widget.remove()
                except Exception:  # noqa: BLE001, S110
                    pass
            # we re-mount by calling compose again — simplest path is refresh
            self.refresh(peripheral=False)
        except Exception:  # noqa: BLE001, S110
            pass


def run(
    initial_url: str | None = None,
    theme_mode: str = "auto",
    cookies: str | None = None,
    cookies_from_browser: str | None = None,
) -> str | None:
    """Module-level entry point — runs the textual app and returns the saved file path."""
    app = ZoinksApp(
        initial_url=initial_url,
        theme_mode=theme_mode,
        cookies=cookies,
        cookies_from_browser=cookies_from_browser,
    )
    app.run()
    return app.last_filepath
