//! TUI app — port of `src/app.tsx`.
//!
//! Single-threaded event loop driven by [`crossterm::event::poll`]. The
//! terminal is fully taken over (alternate screen + raw mode) and restored
//! on exit / panic.
//!
//! State machine mirrors the TS app: input → probing → picking → downloading →
//! done / error. Cancellation is wired through an `Arc<AtomicBool>` shared
//! with the (single) background download thread.

use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;

use crate::clipboard::read_clipboard;
use crate::format::{format_bytes, format_duration, format_eta, format_speed, shorten_path, truncate};
use crate::history::load_history;
use crate::platforms::{detect_platform, is_probably_url, Platform};
use crate::theme::{next_theme_mode, ThemeMode};
use crate::tui::theme::{tui_theme_for, TuiTheme};
use crate::ytdlp::{
    self, build_choices, download, ensure_yt_dlp, find_ffmpeg, probe, DownloadChoice,
    DownloadProgress, VideoInfo,
};

const TAGLINE: &str = "yoink any video. paste. yoink. done.";
const SUBTITLE: &str = "youtube · x · instagram · threads · tiktok · +1800 more";
const YOINK_BUTTON: &str = "yoink";
const DONE_LABEL: &str = "↵ yoink another";

type Term = Terminal<CrosstermBackend<Stdout>>;

#[derive(Debug, Clone)]
enum Phase {
    Input { warning: Option<String> },
    Probing { status: String },
    Picking,
    Downloading {
        choice: DownloadChoice,
        progress: Option<DownloadProgress>,
        processing: bool,
        refreshing: bool,
    },
    Done { filepath: String },
    Error { message: String },
}

impl Phase {
    fn name(&self) -> &'static str {
        match self {
            Phase::Input { .. } => "input",
            Phase::Probing { .. } => "probing",
            Phase::Picking => "picking",
            Phase::Downloading { .. } => "downloading",
            Phase::Done { .. } => "done",
            Phase::Error { .. } => "error",
        }
    }
}

/// A message the background worker thread sends back to the UI thread.
enum WorkerMsg {
    ProbeStatus(String),
    ProbeInfo(VideoInfo, Vec<DownloadChoice>, Option<std::path::PathBuf>),
    DownloadProgress(DownloadProgress),
    DownloadProcessing,
    DownloadDone(String),
    Error(String),
}

pub struct App {
    theme: TuiTheme,
    mode: ThemeMode,
    url_input: String,
    url: String,
    clipboard_url: Option<String>,
    history: Vec<String>,
    platform: Option<Platform>,
    info: Option<VideoInfo>,
    choices: Vec<DownloadChoice>,
    info_json_path: Option<std::path::PathBuf>,
    ytdlp_path: Option<String>,
    ffmpeg_path: Option<String>,
    cookies: Option<std::path::PathBuf>,
    phase: Phase,
    list_state: ListState,
    aborted: Arc<AtomicBool>,
    /// Messages waiting to be drained on the next render tick.
    inbox: Arc<Mutex<Vec<WorkerMsg>>>,
}

impl App {
    pub fn new(initial_url: Option<&str>, theme_mode: &str) -> Self {
        Self::with_cookies(initial_url, theme_mode, None)
    }

    pub fn with_cookies(
        initial_url: Option<&str>,
        theme_mode: &str,
        cookies: Option<&std::path::Path>,
    ) -> Self {
        let mode = ThemeMode::from_str(theme_mode).unwrap_or(ThemeMode::Auto);
        let clipboard_url = if initial_url.is_none() {
            let clipped = read_clipboard();
            let trimmed = clipped.trim();
            if !trimmed.is_empty() && !trimmed.contains(char::is_whitespace) && is_probably_url(trimmed) {
                Some(trimmed.to_string())
            } else {
                None
            }
        } else {
            None
        };
        let phase = match initial_url {
            Some(_) => Phase::Probing { status: "warming up…".into() },
            None => Phase::Input { warning: None },
        };
        Self {
            theme: tui_theme_for(mode),
            mode,
            url_input: String::new(),
            url: initial_url.unwrap_or("").to_string(),
            clipboard_url,
            history: load_history(),
            platform: None,
            info: None,
            choices: Vec::new(),
            info_json_path: None,
            ytdlp_path: None,
            ffmpeg_path: None,
            cookies: cookies.map(|p| p.to_path_buf()),
            phase,
            list_state: ListState::default(),
            aborted: Arc::new(AtomicBool::new(false)),
            inbox: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn cycle_theme(&mut self) {
        self.mode = next_theme_mode(self.mode);
        self.theme = tui_theme_for(self.mode);
    }

    fn reset_to_input(&mut self) {
        self.url.clear();
        self.url_input.clear();
        self.platform = None;
        self.info = None;
        self.choices.clear();
        self.info_json_path = None;
        self.phase = Phase::Input { warning: None };
    }

    fn cancel_run(&mut self) {
        self.aborted.store(true, Ordering::SeqCst);
        let kept_url = self.url.clone();
        self.reset_to_input();
        self.url_input = kept_url;
    }

    fn start_probe(&mut self, url: String) {
        self.aborted.store(false, Ordering::SeqCst);
        self.url = url.clone();
        self.platform = Some(detect_platform(&url));
        self.phase = Phase::Probing { status: "warming up…".into() };
        let ytdlp = self.ytdlp_path.clone();
        let cookies = self.cookies.clone();
        let aborted = self.aborted.clone();
        let inbox = self.inbox.clone();
        let url_cloned = url.clone();
        thread::spawn(move || {
            // Resolve yt-dlp if we haven't already
            let ytdlp = match ytdlp {
                Some(p) => p,
                None => {
                    let mut on_status = |msg: &str| {
                        if let Ok(mut buf) = inbox.lock() {
                            buf.push(WorkerMsg::ProbeStatus(msg.to_string()));
                        }
                    };
                    match ensure_yt_dlp(&mut on_status, &aborted) {
                        Ok(p) => p,
                        Err(e) => {
                            if aborted.load(Ordering::SeqCst) {
                                return;
                            }
                            if let Ok(mut buf) = inbox.lock() {
                                buf.push(WorkerMsg::Error(e));
                            }
                            return;
                        }
                    }
                }
            };
            if aborted.load(Ordering::SeqCst) {
                return;
            }
            if let Ok(mut buf) = inbox.lock() {
                buf.push(WorkerMsg::ProbeStatus("fetching video info…".into()));
            }
            match probe(&ytdlp, &url_cloned, cookies.as_deref(), &aborted) {
                Ok(result) => {
                    let info = result.info.clone();
                    let choices = build_choices(&info);
                    let info_json_path = result.info_json_path.clone();
                    if let Ok(mut buf) = inbox.lock() {
                        buf.push(WorkerMsg::ProbeInfo(info, choices, Some(info_json_path)));
                    }
                }
                Err(e) => {
                    if aborted.load(Ordering::SeqCst) {
                        return;
                    }
                    // If yt-dlp rejected the URL, try updating yt-dlp and
                    // retry once. Common case: the user's bundled yt-dlp is
                    // a few weeks old and the site shipped a new URL scheme.
                    if e.contains("Unsupported URL") || e.contains("unable to extract") {
                        if let Ok(mut buf) = inbox.lock() {
                            buf.push(WorkerMsg::ProbeStatus(
                                "yt-dlp rejected the URL — updating and retrying…".into(),
                            ));
                        }
                        let mut on_status = |msg: &str| {
                            if let Ok(mut buf) = inbox.lock() {
                                buf.push(WorkerMsg::ProbeStatus(msg.to_string()));
                            }
                        };
                        let _ = ytdlp::update_yt_dlp(&ytdlp, &mut on_status);
                        if !aborted.load(Ordering::SeqCst) {
                            match probe(&ytdlp, &url_cloned, cookies.as_deref(), &aborted) {
                                Ok(result) => {
                                    let info = result.info.clone();
                                    let choices = build_choices(&info);
                                    let info_json_path = result.info_json_path.clone();
                                    if let Ok(mut buf) = inbox.lock() {
                                        buf.push(WorkerMsg::ProbeInfo(info, choices, Some(info_json_path)));
                                    }
                                    return;
                                }
                                Err(e2) => {
                                    if let Ok(mut buf) = inbox.lock() {
                                        buf.push(WorkerMsg::Error(e2));
                                    }
                                    return;
                                }
                            }
                        }
                    }
                    if let Ok(mut buf) = inbox.lock() {
                        buf.push(WorkerMsg::Error(e));
                    }
                }
            }
        });
    }

    fn start_download(&mut self, idx: usize) {
        let Some(choice) = self.choices.get(idx).cloned() else {
            return;
        };
        self.aborted.store(false, Ordering::SeqCst);
        self.phase = Phase::Downloading {
            choice: choice.clone(),
            progress: None,
            processing: false,
            refreshing: false,
        };

        let ytdlp = self.ytdlp_path.clone().unwrap_or_else(|| "yt-dlp".into());
        let ffmpeg_location = self.ffmpeg_path.clone().or_else(|| find_ffmpeg());
        // resolve ffmpeg now (system PATH check is sync)
        let ffmpeg_location = ffmpeg_location.or_else(find_ffmpeg);
        let url = self.url.clone();
        let info_json_path = self.info_json_path.clone();
        let cookies = self.cookies.clone();
        let aborted = self.aborted.clone();
        let inbox = self.inbox.clone();
        let out_dir = crate::default_out_dir();

        thread::spawn(move || {
            // First attempt: try with cached info.json (fast path — skips
            // re-extraction). Media URLs in the cached info can expire within
            // minutes, so a failure here is recoverable.
            let opts = ytdlp::DownloadOpts {
                ytdlp: ytdlp.clone(),
                ffmpeg_location: ffmpeg_location.clone(),
                url: url.clone(),
                info_json_path: info_json_path.clone(),
                choice: choice.clone(),
                out_dir: out_dir.clone(),
                cookies: cookies.clone(),
            };
            let inbox_for_progress = inbox.clone();
            let inbox_for_processing = inbox.clone();
            let mut handlers = ytdlp::DownloadHandlers {
                on_progress: Box::new(move |p| {
                    if let Ok(mut buf) = inbox_for_progress.lock() {
                        buf.push(WorkerMsg::DownloadProgress(p));
                    }
                }),
                on_processing: Box::new(move || {
                    if let Ok(mut buf) = inbox_for_processing.lock() {
                        buf.push(WorkerMsg::DownloadProcessing);
                    }
                }),
            };

            match download(&opts, &mut handlers, &aborted) {
                Ok(path) => {
                    if let Ok(mut buf) = inbox.lock() {
                        buf.push(WorkerMsg::DownloadDone(path));
                    }
                    return;
                }
                Err(_e) => {
                    if aborted.load(Ordering::SeqCst) {
                        return;
                    }
                    // Retry once with a fresh probe — the cached media URLs
                    // probably expired, OR the format selector didn't match.
                    // Re-extracting metadata may surface newer formats too.
                    if let Ok(mut buf) = inbox.lock() {
                        buf.push(WorkerMsg::DownloadProgress(DownloadProgress {
                            downloaded_bytes: 0,
                            total_bytes: None,
                            speed: None,
                            eta: None,
                            part: 0,
                            total_parts: 1,
                        }));
                    }
                    // signal "refreshing" UI state
                    if let Ok(mut buf) = inbox.lock() {
                        buf.push(WorkerMsg::DownloadProcessing);
                    }
                    let opts_retry = ytdlp::DownloadOpts {
                        ytdlp: ytdlp.clone(),
                        ffmpeg_location,
                        url: url.clone(),
                        info_json_path: None, // force re-extraction
                        choice,
                        out_dir,
                        cookies,
                    };
                    let inbox_for_progress2 = inbox.clone();
                    let inbox_for_processing2 = inbox.clone();
                    let mut handlers2 = ytdlp::DownloadHandlers {
                        on_progress: Box::new(move |p| {
                            if let Ok(mut buf) = inbox_for_progress2.lock() {
                                buf.push(WorkerMsg::DownloadProgress(p));
                            }
                        }),
                        on_processing: Box::new(move || {
                            if let Ok(mut buf) = inbox_for_processing2.lock() {
                                buf.push(WorkerMsg::DownloadProcessing);
                            }
                        }),
                    };
                    match download(&opts_retry, &mut handlers2, &aborted) {
                        Ok(path) => {
                            if let Ok(mut buf) = inbox.lock() {
                                buf.push(WorkerMsg::DownloadDone(path));
                            }
                        }
                        Err(e2) => {
                            if aborted.load(Ordering::SeqCst) {
                                return;
                            }
                            if let Ok(mut buf) = inbox.lock() {
                                buf.push(WorkerMsg::Error(e2));
                            }
                        }
                    }
                }
            }
        });
    }

    fn drain_inbox(&mut self) {
        let messages: Vec<WorkerMsg> = {
            let mut buf = self.inbox.lock().expect("inbox poisoned");
            std::mem::take(&mut *buf)
        };
        for msg in messages {
            match msg {
                WorkerMsg::ProbeStatus(s) => {
                    if let Phase::Probing { status } = &mut self.phase {
                        *status = s;
                    }
                }
                WorkerMsg::ProbeInfo(info, choices, info_json_path) => {
                    self.info = Some(info);
                    self.choices = choices;
                    self.info_json_path = info_json_path;
                    self.list_state.select(Some(0));
                    self.phase = Phase::Picking;
                }
                WorkerMsg::DownloadProgress(p) => {
                    if let Phase::Downloading { progress, .. } = &mut self.phase {
                        *progress = Some(p);
                    }
                }
                WorkerMsg::DownloadProcessing => {
                    if let Phase::Downloading { processing, .. } = &mut self.phase {
                        *processing = true;
                    }
                }
                WorkerMsg::DownloadDone(filepath) => {
                    let url = self.url.clone();
                    self.history = crate::history::add_to_history(&url);
                    self.phase = Phase::Done { filepath };
                }
                WorkerMsg::Error(message) => {
                    self.phase = Phase::Error { message };
                }
            }
        }
    }
}

/// Run the app to completion. Restores the terminal on exit / panic.
pub fn run(initial_url: Option<&str>, theme_mode: &str) -> io::Result<Option<String>> {
    run_with_cookies(initial_url, theme_mode, None)
}

pub fn run_with_cookies(
    initial_url: Option<&str>,
    theme_mode: &str,
    cookies: Option<&std::path::Path>,
) -> io::Result<Option<String>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_loop(&mut terminal, initial_url, theme_mode, cookies)
    }));

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    match result {
        Ok(Ok(path)) => Ok(path),
        Ok(Err(e)) => Err(e),
        Err(payload) => {
            eprintln!("zoinks panicked: {payload:?}");
            Ok(None)
        }
    }
}

fn run_loop(
    terminal: &mut Term,
    initial_url: Option<&str>,
    theme_mode: &str,
    cookies: Option<&std::path::Path>,
) -> io::Result<Option<String>> {
    let mut app = App::with_cookies(initial_url, theme_mode, cookies);
    if initial_url.is_some() {
        app.start_probe(initial_url.unwrap().to_string());
    }

    let mut last_filepath: Option<String> = None;

    loop {
        // 1. drain worker messages
        app.drain_inbox();

        // 2. draw the current frame
        terminal.draw(|f| render(f, &mut app))?;

        // 3. poll for input (non-blocking, so worker messages get a chance)
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if !handle_key(&mut app, key) {
                    break;
                }
            }
        }

        if let Phase::Done { filepath } = &app.phase {
            last_filepath = Some(filepath.clone());
        }
    }
    Ok(last_filepath)
}

fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    // Ctrl-T cycles theme in any phase
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('t') {
        app.cycle_theme();
        return true;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return false;
    }

    match &app.phase {
        Phase::Input { .. } => match key.code {
            KeyCode::Enter => {
                let value = app.url_input.trim().to_string();
                if !is_probably_url(&value) {
                    app.phase = Phase::Input {
                        warning: Some("that doesn't look like a link — paste a full url".into()),
                    };
                } else {
                    app.start_probe(value);
                }
            }
            KeyCode::Esc => {
                app.url_input.clear();
            }
            KeyCode::Backspace => {
                app.url_input.pop();
            }
            KeyCode::Tab => {
                if let Some(cb) = &app.clipboard_url {
                    if app.url_input.is_empty() {
                        app.url_input = cb.clone();
                    }
                }
            }
            KeyCode::Char(c) => {
                app.url_input.push(c);
            }
            _ => {}
        },
        Phase::Probing { .. } => {
            if key.code == KeyCode::Esc {
                app.cancel_run();
            }
        }
        Phase::Picking => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let i = app.list_state.selected().unwrap_or(0);
                let next = if i == 0 { app.choices.len() - 1 } else { i - 1 };
                app.list_state.select(Some(next));
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let i = app.list_state.selected().unwrap_or(0);
                let next = (i + 1) % app.choices.len();
                app.list_state.select(Some(next));
            }
            KeyCode::Enter => {
                if let Some(i) = app.list_state.selected() {
                    app.start_download(i);
                }
            }
            KeyCode::Esc => app.reset_to_input(),
            _ => {}
        },
        Phase::Downloading { .. } => {
            if key.code == KeyCode::Esc {
                app.cancel_run();
            }
        }
        Phase::Done { .. } | Phase::Error { .. } => {
            if key.code == KeyCode::Enter || key.code == KeyCode::Esc {
                app.reset_to_input();
            }
        }
    }
    true
}

fn render(f: &mut ratatui::Frame<'_>, app: &mut App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // logo
            Constraint::Length(2), // tagline + subtitle
            Constraint::Min(5),    // main content
            Constraint::Length(2), // footer hints
        ])
        .split(area);

    render_logo(f, chunks[0], app);
    render_tagline(f, chunks[1], app);
    render_main(f, chunks[2], app);
    render_footer(f, chunks[3], app);
}

fn render_logo(f: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let art = vec![
        "▓ ▓ █▀█ ▀█▀ █▀▄█ █ █ █▀▀",
        "▀█▀ █ ▓  ▓  █  ▓ ▓▀▄ ▀▀▓",
        " ▀  ▀▀▀ ▀▀▀ ▀  ▀ ▀ ▀ ▀▀▀",
    ];
    let lines: Vec<Line> = art
        .iter()
        .map(|row| Line::from(Span::styled(*row, app.theme.primary_style())))
        .collect();
    let p = Paragraph::new(lines).alignment(Alignment::Center);
    f.render_widget(p, area);
}

fn render_tagline(f: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let lines = vec![
        Line::from(Span::styled(TAGLINE, app.theme.primary_style())),
        Line::from(Span::styled(SUBTITLE, app.theme.gray_style())),
    ];
    let p = Paragraph::new(lines).alignment(Alignment::Center);
    f.render_widget(p, area);
}

fn render_main(f: &mut ratatui::Frame<'_>, area: Rect, app: &mut App) {
    let centered = center_widget(area, 70);
    match &app.phase {
        Phase::Input { warning } => {
            let title = if app.history.is_empty() {
                "Paste a link"
            } else {
                "Paste a link  ·  ↑ for history"
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(app.theme.gray_style())
                .title(Span::styled(format!(" {title} "), app.theme.primary_style()));
            let inner = block.inner(centered);
            f.render_widget(block, centered);

            let input_line = if app.url_input.is_empty() {
                Span::styled(
                    "https://youtube.com/watch?v=…",
                    app.theme.gray_style(),
                )
            } else {
                Span::styled(&app.url_input, app.theme.primary_style())
            };
            let mut lines = vec![
                Line::from(""),
                Line::from(input_line),
                Line::from(""),
                Line::from(Span::styled(
                    format!("  {YOINK_BUTTON}  "),
                    Style::default()
                        .fg(Color::Black)
                        .bg(app.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )),
            ];
            if let Some(w) = warning {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(format!("✗ {w}"), app.theme.gray_style())));
            } else if let Some(cb) = &app.clipboard_url {
                if app.url_input.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!("link in your clipboard — ⇥ to paste it: {cb}"),
                        app.theme.gray_style(),
                    )));
                }
            }
            f.render_widget(Paragraph::new(lines), inner);
        }
        Phase::Probing { status } => {
            let platform_label = app
                .platform
                .as_ref()
                .map(|p| p.label.clone())
                .unwrap_or_else(|| "Paste a link".into());
            let url_display = truncate(&app.url, 50);
            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(url_display, app.theme.gray_style())),
                Line::from(""),
                Line::from(Span::styled(
                    format!("  ⠋ {status}  "),
                    app.theme.primary_style(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!("▸ {platform_label}"),
                    app.theme.gray_style(),
                )),
            ];
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(app.theme.gray_style())
                .title(Span::styled(" yoink ", app.theme.primary_style()));
            let inner = block.inner(centered);
            f.render_widget(block, centered);
            f.render_widget(Paragraph::new(lines), inner);
        }
        Phase::Picking => {
            let platform = app.platform.as_ref();
            let info = app.info.as_ref();
            // Left: title + meta
            let title_lines: Vec<Line> = info
                .map(|i| {
                    crate::format::wrap_text(&i.title, 30)
                        .into_iter()
                        .map(|l| Line::from(Span::styled(l, app.theme.primary_style().add_modifier(Modifier::BOLD))))
                        .collect()
                })
                .unwrap_or_default();
            let meta_line = if let (Some(p), Some(i)) = (platform, info) {
                let mut bits = vec![format!("▸ {}", p.label)];
                if let Some(d) = i.duration {
                    if d > 0.0 {
                        bits.push(format_duration(d));
                    }
                }
                if let Some(u) = &i.uploader {
                    bits.push(u.clone());
                }
                Some(bits.join(" · "))
            } else {
                None
            };

            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(1)])
                .split(centered);

            let left_para = Paragraph::new(title_lines).wrap(Wrap { trim: false });
            f.render_widget(left_para, left_chunks[0]);
            if let Some(m) = meta_line {
                let p = Paragraph::new(Line::from(Span::styled(m, app.theme.gray_style())));
                f.render_widget(p, left_chunks[1]);
            }

            // Right: download list
            let list_area = Rect::new(
                centered.x + centered.width.saturating_sub(40),
                centered.y,
                40,
                centered.height,
            );
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(app.theme.gray_style())
                .title(Span::styled(" Download ", app.theme.primary_style()));
            let items: Vec<ListItem> = app
                .choices
                .iter()
                .map(|c| {
                    let prefix = if c.kind == ytdlp::ChoiceKind::Audio { "♪ " } else { "▶ " };
                    ListItem::new(Line::from(Span::styled(
                        format!("{prefix}{}", c.label),
                        app.theme.primary_style(),
                    )))
                })
                .collect();
            let list = List::new(items)
                .block(block)
                .highlight_style(
                    Style::default()
                        .fg(app.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("❯ ");
            f.render_stateful_widget(list, list_area, &mut app.list_state);
        }
        Phase::Downloading { choice, progress, processing, refreshing } => {
            let title = app.info.as_ref().map(|i| truncate(&i.title, 42)).unwrap_or_default();
            let mut lines = vec![
                Line::from(Span::styled(
                    format!("{title} · {}", choice.label),
                    app.theme.gray_style(),
                )),
                Line::from(""),
            ];

            if *processing {
                lines.push(progress_bar_line(1.0, app));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "⠋ processing…",
                    app.theme.primary_style(),
                )));
            } else if let Some(p) = progress {
                if let Some(total) = p.total_bytes {
                    let pct = if total > 0 { p.downloaded_bytes as f64 / total as f64 } else { 0.0 };
                    lines.push(progress_bar_line(pct, app));
                    lines.push(Line::from(""));
                    let speed = p.speed.filter(|&s| s > 0.0).map(format_speed).unwrap_or_default();
                    let eta = p.eta.filter(|&e| e > 0.0).map(|e| format!("{} left", format_eta(e))).unwrap_or_default();
                    lines.push(Line::from(Span::styled(
                        format!("{speed:>10}  {eta:<12}"),
                        app.theme.gray_style(),
                    )));
                } else {
                    let bytes = format_bytes(p.downloaded_bytes as f64);
                    let speed = p.speed.filter(|&s| s > 0.0).map(format_speed).unwrap_or_default();
                    lines.push(Line::from(Span::styled(
                        "⠋ downloading…",
                        app.theme.primary_style(),
                    )));
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!("{bytes:>8}  {speed:<10}"),
                        app.theme.gray_style(),
                    )));
                }
            } else {
                lines.push(progress_bar_line(0.0, app));
                lines.push(Line::from(""));
                let msg = if *refreshing { " link expired — grabbing a fresh one…" } else { " starting download…" };
                lines.push(Line::from(Span::styled(
                    format!("⠋{msg}"),
                    app.theme.primary_style(),
                )));
            }
            let p = Paragraph::new(lines).alignment(Alignment::Center);
            f.render_widget(p, centered);
        }
        Phase::Done { filepath } => {
            let home = directories::BaseDirs::new()
                .map(|b| b.home_dir().to_string_lossy().into_owned())
                .unwrap_or_default();
            let short = shorten_path(filepath, &home, 60);
            let lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("✓ yoinked! ", app.theme.primary_style().add_modifier(Modifier::BOLD)),
                    Span::styled("find your file in:", app.theme.primary_style()),
                ]),
                Line::from(Span::styled(short, app.theme.gray_style())),
                Line::from(""),
                Line::from(Span::styled(
                    format!("  {DONE_LABEL}  "),
                    Style::default()
                        .fg(Color::Black)
                        .bg(app.theme.primary)
                        .add_modifier(Modifier::BOLD),
                )),
            ];
            let p = Paragraph::new(lines).alignment(Alignment::Center);
            f.render_widget(p, centered);
        }
        Phase::Error { message } => {
            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("✗ {message}"),
                    app.theme.primary_style().add_modifier(Modifier::BOLD),
                )),
            ];
            let p = Paragraph::new(lines).alignment(Alignment::Center);
            f.render_widget(p, centered);
        }
    }
}

fn progress_bar_line(pct: f64, app: &App) -> Line<'_> {
    let width = 30usize;
    let filled = ((pct.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    let bar: String = "█".repeat(filled) + &"░".repeat(width - filled);
    let pct_label = format!(" {:3.0}% ", pct * 100.0);
    Line::from(vec![
        Span::styled(bar, app.theme.primary_style()),
        Span::styled(pct_label, app.theme.gray_style()),
    ])
}

fn render_footer(f: &mut ratatui::Frame<'_>, area: Rect, app: &App) {
    let mut hints: Vec<(&str, &str)> = Vec::new();
    match app.phase.name() {
        "input" => {
            hints.push(("↵", "yoink"));
            hints.push(("^c", "quit"));
            if !app.history.is_empty() {
                hints.push(("↑", "history"));
            }
        }
        "probing" => {
            hints.push(("esc", "cancel"));
            hints.push(("^c", "quit"));
        }
        "picking" => {
            hints.push(("↑↓", "choose"));
            hints.push(("↵", "yoink"));
            hints.push(("esc", "back"));
            hints.push(("^c", "quit"));
        }
        "downloading" => {
            hints.push(("esc", "cancel"));
            hints.push(("^c", "quit"));
        }
        "done" => {
            hints.push(("↵", "yoink another"));
            hints.push(("^c", "quit"));
        }
        "error" => {
            hints.push(("↵", "try again"));
            hints.push(("^c", "quit"));
        }
        _ => {}
    }
    hints.push(("^t", "theme"));

    let spans: Vec<Span> = hints
        .iter()
        .flat_map(|(k, v)| {
            vec![
                Span::styled(format!(" {k} "), Style::default().add_modifier(Modifier::BOLD).fg(app.theme.primary)),
                Span::styled(format!("{v}  "), app.theme.gray_style()),
            ]
        })
        .collect();
    let line = Line::from(spans);
    let p = Paragraph::new(line).alignment(Alignment::Center);
    f.render_widget(p, area);
}

fn center_widget(area: Rect, width: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let x = area.x + (area.width - w) / 2;
    Rect::new(x, area.y + 1, w, area.height.saturating_sub(2))
}
