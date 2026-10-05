# Contributing to zoinks (Python + Rust)

Thanks for being interested — patches welcome.

## Project layout

See the README's "Project layout" section for the directory map. The
important bits:

- **Rust source** lives in `src/` — every module is a near 1:1 port of the
  TS original. Module docstrings point at the upstream file (`src/lib/ytdlp.ts`
  etc.) so you can cross-reference.
- **Python source** lives in `python/zoinks/` — the textual TUI imports the
  Rust core via `from ._zoinks_core import …`. The Rust core is built by
  maturin and shipped inside the wheel as `zoinks/_zoinks_core*.so`.
- **Tests** live alongside the code they cover: `#[cfg(test)] mod tests`
  blocks in each Rust file, and `tests/test_*.py` for the Python side.

## Build

### Rust binary (no Python needed)

```sh
cargo build --no-default-features --bin zoinks-tui
cargo test --no-default-features --lib
./target/debug/zoinks-tui --help
./target/debug/zoinks-tui https://youtu.be/dQw4w9WgXcQ
```

### Python package (PyO3 + textual)

```sh
# 1. install maturin + textual in your venv
pip install maturin textual pytest ruff

# 2. build the Rust core and install it (editable) into the venv
maturin develop --release

# 3. run the textual TUI
zoinks                            # or: python -m zoinks

# 4. run tests
pytest tests/
ruff check python/zoinks/
```

### Both at once

```sh
cargo test --no-default-features --lib && maturin develop --release && pytest tests/
```

## Releasing

1. **Bump versions** in lock-step:
   - `Cargo.toml` → `version = "..."`
   - `pyproject.toml` → `version = "..."`
   - `CHANGELOG.md` → add a new section
2. **Run the full test suite**:
   ```sh
   cargo test --no-default-features --lib
   maturin develop --release
   pytest tests/
   ```
3. **Build artifacts**:
   ```sh
   cargo build --release --no-default-features --bin zoinks-tui
   maturin build --release
   ```
   - Standalone binary lands at `target/release/zoinks-tui`
   - Wheel lands at `dist/zoinks-<version>-*.whl`
4. **Tag and push**:
   ```sh
   git tag v0.4.1
   git push origin v0.4.1
   ```
   This triggers the GitHub Actions release workflow that cross-builds every
   platform target and uploads both the binaries and the wheels to the
   GitHub Releases page.

## Code style

### Rust

- `cargo fmt` before commit.
- Public functions have doc comments — link to the upstream TS file in the
  module-level docstring so reviewers can diff against the source.
- Prefer `Result<T, String>` over `anyhow::Error` / `thiserror` — we have
  one consumer (the TUI) and it just prints the error string, so we save a
  dependency.
- Use `Arc<AtomicBool>` for cancellation, not channels — simpler, and
  matches the original TS `AbortSignal` semantics.

### Python

- `ruff check python/zoinks/` — keep it clean.
- Type hints everywhere. The Rust core types flow through automatically
  (PyO3 generates stubs); use `mypy` or `pyright` if you want to enforce.
- The textual app is intentionally simple — we don't try to reproduce the
  mouse click hit-testing of the original Ink app. Keyboard + tab focus
  is enough; the original was a single-developer hobby project too.

## Testing yt-dlp integration locally

CI mocks `yt-dlp`. To run the full integration test locally:

```sh
# ensure yt-dlp is installed (the Rust code will auto-download it on first run)
cargo run --no-default-features --bin zoinks-tui -- https://youtu.be/dQw4w9WgXcQ
```

If a test fails because yt-dlp is offline or the URL is geo-blocked, mark
the test `#[ignore]` and explain in the comment.

## Cutting a patch

- Branch off `main`.
- One logical change per PR.
- Update `CHANGELOG.md` under `[Unreleased]`.
- If you touch the public Python API, regenerate the type stubs:
  ```sh
  maturin develop --release && stubgen -p zoinks -o python/zoinks/py.typed
  ```

## License

By contributing, you agree that your contributions will be licensed under
the [MIT license](LICENSE) — same as upstream.
