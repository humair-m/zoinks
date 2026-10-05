//! PyO3 bindings — exposes the Rust core to Python as `zoinks._zoinks_core`.
//!
//! The shape mirrors the TS public surface so a Python user can:
//!
//! ```python
//! from zoinks import Zoinks
//! y = Zoinks()
//! info = y.probe("https://youtu.be/...")
//! choices = y.build_choices(info)
//! y.download(url, choices[0], on_progress=lambda p: print(p))
//! ```
//!
//! Blocking work (`probe`, `download`, `ensure_yt_dlp`) releases the GIL for
//! the duration of the underlying yt-dlp call so a Python async caller can
//! run other coroutines in parallel.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::ytdlp::{
    self, build_choices, download, ensure_yt_dlp, find_ffmpeg, probe, DownloadChoice,
    DownloadOpts, DownloadProgress, VideoInfo,
};
use crate::{add_to_history, load_history, read_clipboard, VERSION};
use crate::platforms::{detect_platform, is_probably_url, Platform as CorePlatform};

#[pyclass(name = "Platform")]
#[derive(Clone)]
struct PyPlatform {
    inner: CorePlatform,
}

#[pymethods]
impl PyPlatform {
    #[getter]
    fn key(&self) -> &str {
        &self.inner.key
    }
    #[getter]
    fn label(&self) -> &str {
        &self.inner.label
    }
    fn __repr__(&self) -> String {
        format!("<Platform key={:?} label={:?}>", self.inner.key, self.inner.label)
    }
}

impl From<CorePlatform> for PyPlatform {
    fn from(p: CorePlatform) -> Self {
        Self { inner: p }
    }
}

#[pyclass(name = "VideoInfo")]
struct PyVideoInfo {
    #[pyo3(get)]
    title: String,
    #[pyo3(get)]
    uploader: Option<String>,
    #[pyo3(get)]
    duration: Option<f64>,
    #[pyo3(get)]
    webpage_url: Option<String>,
    #[pyo3(get)]
    extractor_key: Option<String>,
    #[pyo3(get, set)]
    formats: Vec<PyFormat>,
    /// Path to a temp file with the raw `-J` output — pass it to
    /// `Zoinks.download(..., info_json_path=...)` to skip re-extraction.
    #[pyo3(get)]
    info_json_path: Option<String>,
}

#[pyclass(name = "Format")]
#[derive(Clone)]
struct PyFormat {
    #[pyo3(get)]
    format_id: String,
    #[pyo3(get)]
    ext: Option<String>,
    #[pyo3(get)]
    vcodec: Option<String>,
    #[pyo3(get)]
    acodec: Option<String>,
    #[pyo3(get)]
    height: Option<u32>,
    #[pyo3(get)]
    width: Option<u32>,
    #[pyo3(get)]
    abr: Option<f64>,
    #[pyo3(get)]
    tbr: Option<f64>,
    #[pyo3(get)]
    filesize: Option<u64>,
    #[pyo3(get)]
    filesize_approx: Option<u64>,
}

impl From<ytdlp::RawFormat> for PyFormat {
    fn from(f: ytdlp::RawFormat) -> Self {
        Self {
            format_id: f.format_id,
            ext: f.ext,
            vcodec: f.vcodec,
            acodec: f.acodec,
            height: f.height,
            width: f.width,
            abr: f.abr,
            tbr: f.tbr,
            filesize: f.filesize,
            filesize_approx: f.filesize_approx,
        }
    }
}

#[pyclass(name = "DownloadChoice")]
#[derive(Clone)]
struct PyDownloadChoice {
    #[pyo3(get, set)]
    label: String,
    #[pyo3(get, set)]
    kind: String, // "video" | "audio"
    #[pyo3(get, set)]
    args: Vec<String>,
}

impl From<DownloadChoice> for PyDownloadChoice {
    fn from(c: DownloadChoice) -> Self {
        Self {
            label: c.label,
            kind: match c.kind {
                ytdlp::ChoiceKind::Video => "video",
                ytdlp::ChoiceKind::Audio => "audio",
            }
            .to_string(),
            args: c.args,
        }
    }
}

impl From<&PyDownloadChoice> for DownloadChoice {
    fn from(c: &PyDownloadChoice) -> Self {
        DownloadChoice {
            label: c.label.clone(),
            kind: match c.kind.as_str() {
                "audio" => ytdlp::ChoiceKind::Audio,
                _ => ytdlp::ChoiceKind::Video,
            },
            args: c.args.clone(),
        }
    }
}

/// Main entry point — call `Zoinks()` and use its methods.
///
/// Example::
///
///     >>> from zoinks import Zoinks
///     >>> y = Zoinks()
///     >>> y.ensure_ytdlp()
///     '/home/user/.zoinks/bin/yt-dlp'
///     >>> info = y.probe("https://youtu.be/dQw4w9WgXcQ")
///     >>> choices = y.build_choices(info)
///     >>> y.download(info, choices[0], on_progress=lambda p: print(p))
///     '/home/user/Downloads/Rick Astley - Never Gonna Give You Up.mp4'
#[pyclass(name = "Zoinks")]
struct PyZoinks {
    ytdlp_path: Option<String>,
    ffmpeg_path: Option<String>,
    aborted: Arc<AtomicBool>,
}

#[pymethods]
impl PyZoinks {
    #[new]
    fn new() -> Self {
        Self {
            ytdlp_path: None,
            ffmpeg_path: None,
            aborted: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Cancel any in-flight probe/download. Safe to call from another thread.
    fn abort(&self) {
        self.aborted.store(true, Ordering::SeqCst);
    }

    /// Reset the abort flag after a cancelled run.
    fn reset_abort(&self) {
        self.aborted.store(false, Ordering::SeqCst);
    }

    /// Make sure yt-dlp is available — returns the binary path on success,
    /// raises `RuntimeError` on failure.
    ///
    /// Pass a `status_callback(str) -> None` to be notified of one-line
    /// progress messages (e.g. "first run: fetching yt-dlp…").
    #[pyo3(signature = (status_callback=None))]
    fn ensure_ytdlp(
        &mut self,
        py: Python<'_>,
        status_callback: Option<PyObject>,
    ) -> PyResult<String> {
        let aborted = self.aborted.clone();
        // ensure_yt_dlp runs on this thread (no `allow_threads`) so we can
        // call back into Python synchronously when status messages fire.
        let mut on_status = |msg: &str| {
            if let Some(cb) = status_callback.as_ref() {
                let _ = cb.call1(py, (msg,));
            }
        };
        let path = ensure_yt_dlp(&mut on_status, &aborted)
            .map_err(PyRuntimeError::new_err)?;
        self.ytdlp_path = Some(path.clone());
        Ok(path)
    }

    /// Resolve ffmpeg if it's not already on PATH. Cached on the instance.
    fn find_ffmpeg(&mut self) -> Option<String> {
        if self.ffmpeg_path.is_some() {
            return self.ffmpeg_path.clone();
        }
        let p = find_ffmpeg();
        self.ffmpeg_path = p.clone();
        p
    }

    /// Run `yt-dlp -U` against the resolved yt-dlp binary. Useful when the
    /// user hits "Unsupported URL" — typically means the bundled yt-dlp is
    /// out of date. If self-update is unavailable (some standalone builds),
    /// re-downloads the latest release instead.
    fn update_ytdlp(&self, py: Python<'_>) -> PyResult<()> {
        let ytdlp = self.ytdlp_path.clone().unwrap_or_else(|| "yt-dlp".into());
        py.allow_threads(|| {
            let mut on_status = |_: &str| {};
            ytdlp::update_yt_dlp(&ytdlp, &mut on_status)
        })
        .map_err(PyRuntimeError::new_err)
    }

    /// Probe a URL. Returns a [`PyVideoInfo`] holding the parsed metadata
    /// plus the path to the temp info.json file (for `download`'s
    /// `info_json_path` argument).
    ///
    /// Pass `cookies` as a path string to a Netscape-format cookies file
    /// for sites that require login (X, Facebook, Instagram).
    ///
    /// Pass `cookies_from_browser` as a browser name (chrome, firefox,
    /// safari, edge, opera, chromium, brave, vivaldi, whale) to auto-pull
    /// cookies from that browser's store — easiest option for YouTube's
    /// bot detection. Mutually exclusive with `cookies`.
    #[pyo3(signature = (url, cookies=None, cookies_from_browser=None))]
    fn probe(
        &self,
        py: Python<'_>,
        url: &str,
        cookies: Option<String>,
        cookies_from_browser: Option<String>,
    ) -> PyResult<PyVideoInfo> {
        let ytdlp = self.ytdlp_path.clone().unwrap_or_else(|| "yt-dlp".into());
        let aborted = self.aborted.clone();
        let url = url.to_string();
        let cookies_path = cookies.map(std::path::PathBuf::from);
        let result = py
            .allow_threads(move || {
                probe(
                    &ytdlp,
                    &url,
                    cookies_path.as_deref(),
                    cookies_from_browser.as_deref(),
                    &*aborted,
                )
            })
            .map_err(PyRuntimeError::new_err)?;
        Ok(PyVideoInfo {
            title: result.info.title,
            uploader: result.info.uploader,
            duration: result.info.duration,
            webpage_url: result.info.webpage_url,
            extractor_key: result.info.extractor_key,
            formats: result.info.formats.into_iter().map(PyFormat::from).collect(),
            info_json_path: Some(result.info_json_path.to_string_lossy().into_owned()),
        })
    }

    /// Build the resolution/audio picker choices from a [`PyVideoInfo`].
    /// Returns a list of [`PyDownloadChoice`] suitable for display.
    #[pyo3(signature = (info,))]
    fn build_choices(&self, info: &PyVideoInfo) -> Vec<PyDownloadChoice> {
        let core_info = VideoInfo {
            title: info.title.clone(),
            uploader: info.uploader.clone(),
            duration: info.duration,
            webpage_url: info.webpage_url.clone(),
            extractor_key: info.extractor_key.clone(),
            formats: info
                .formats
                .iter()
                .map(|f| ytdlp::RawFormat {
                    format_id: f.format_id.clone(),
                    ext: f.ext.clone(),
                    vcodec: f.vcodec.clone(),
                    acodec: f.acodec.clone(),
                    height: f.height,
                    width: f.width,
                    abr: f.abr,
                    tbr: f.tbr,
                    filesize: f.filesize,
                    filesize_approx: f.filesize_approx,
                })
                .collect(),
        };
        build_choices(&core_info)
            .into_iter()
            .map(PyDownloadChoice::from)
            .collect()
    }

    /// Download a video. Pass an `on_progress(dict)` and/or `on_processing()`
    /// callback to be notified of progress / merge phase.
    ///
    /// Positional arguments:
    ///   url         — the video URL (ignored when `info_json_path` is set)
    ///   choice      — a `PyDownloadChoice` from `build_choices`
    ///
    /// Keyword arguments:
    ///   out_dir              — destination dir (default: `~/Downloads`)
    ///   info_json_path       — reuse the probe's cached metadata
    ///   cookies              — path to a Netscape-format cookies file
    ///   cookies_from_browser — browser name (chrome, firefox, safari, edge,
    ///                          opera, chromium, brave, vivaldi, whale) —
    ///                          auto-pulls cookies from the browser's store.
    ///                          Easiest fix for YouTube's bot detection.
    ///   on_progress          — `callable(dict)` — bytes, speed, eta, etc.
    ///   on_processing        — `callable()` — called when yt-dlp starts merging
    #[pyo3(signature = (url, choice, out_dir=None, info_json_path=None, cookies=None, cookies_from_browser=None, on_progress=None, on_processing=None))]
    fn download(
        &self,
        py: Python<'_>,
        url: &str,
        choice: &PyDownloadChoice,
        out_dir: Option<String>,
        info_json_path: Option<String>,
        cookies: Option<String>,
        cookies_from_browser: Option<String>,
        on_progress: Option<PyObject>,
        on_processing: Option<PyObject>,
    ) -> PyResult<String> {
        let ytdlp = self.ytdlp_path.clone().unwrap_or_else(|| "yt-dlp".into());
        let ffmpeg_location = self.ffmpeg_path.clone();
        let out_dir = out_dir
            .map(PathBuf::from)
            .unwrap_or_else(crate::default_out_dir);
        let info_json_path = info_json_path.map(PathBuf::from);
        let cookies = cookies.map(PathBuf::from);
        let aborted = self.aborted.clone();
        let opts = DownloadOpts {
            ytdlp: ytdlp.clone(),
            ffmpeg_location,
            url: url.to_string(),
            info_json_path,
            choice: choice.into(),
            out_dir,
            cookies,
            cookies_from_browser,
        };

        let mut handlers = ytdlp::DownloadHandlers {
            on_progress: Box::new(move |p: DownloadProgress| {
                if let Some(cb) = on_progress.as_ref() {
                    let _ = Python::with_gil(|py| {
                        let dict = PyDict::new_bound(py);
                        let _ = dict.set_item("downloaded_bytes", p.downloaded_bytes);
                        let _ = dict.set_item(
                            "total_bytes",
                            p.total_bytes.unwrap_or(0),
                        );
                        let _ = dict.set_item("speed", p.speed.unwrap_or(0.0));
                        let _ = dict.set_item("eta", p.eta.unwrap_or(0.0));
                        let _ = dict.set_item("part", p.part);
                        let _ = dict.set_item("total_parts", p.total_parts);
                        cb.call1(py, (dict,))
                    });
                }
            }),
            on_processing: Box::new(move || {
                if let Some(cb) = on_processing.as_ref() {
                    let _ = Python::with_gil(|py| cb.call0(py));
                }
            }),
        };

        py.allow_threads(move || download(&opts, &mut handlers, &*aborted))
            .map_err(PyRuntimeError::new_err)
    }
}

/// Top-level helpers exposed as `zoinks._zoinks_core.<name>`.
#[pyfunction]
fn detect_platform_py(url: &str) -> PyPlatform {
    detect_platform(url).into()
}

#[pyfunction]
fn is_probably_url_py(input: &str) -> bool {
    is_probably_url(input)
}

#[pyfunction]
fn read_clipboard_py() -> String {
    read_clipboard()
}

#[pyfunction]
fn load_history_py() -> Vec<String> {
    load_history()
}

#[pyfunction]
fn add_to_history_py(url: &str) -> Vec<String> {
    add_to_history(url)
}

#[pyfunction]
fn version() -> &'static str {
    VERSION
}

#[pyfunction]
fn default_out_dir_py() -> String {
    crate::default_out_dir().to_string_lossy().into_owned()
}

#[pyfunction]
fn zoinks_bin_dir_py() -> String {
    crate::zoinks_bin_dir().to_string_lossy().into_owned()
}

#[pymodule]
fn _zoinks_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", VERSION)?;
    m.add_class::<PyPlatform>()?;
    m.add_class::<PyVideoInfo>()?;
    m.add_class::<PyFormat>()?;
    m.add_class::<PyDownloadChoice>()?;
    m.add_class::<PyZoinks>()?;
    m.add_function(wrap_pyfunction!(detect_platform_py, m)?)?;
    m.add_function(wrap_pyfunction!(is_probably_url_py, m)?)?;
    m.add_function(wrap_pyfunction!(read_clipboard_py, m)?)?;
    m.add_function(wrap_pyfunction!(load_history_py, m)?)?;
    m.add_function(wrap_pyfunction!(add_to_history_py, m)?)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(default_out_dir_py, m)?)?;
    m.add_function(wrap_pyfunction!(zoinks_bin_dir_py, m)?)?;
    Ok(())
}
