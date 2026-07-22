# Docs Index

Read in this order when getting oriented:

1. `README.md`
2. `AGENTS.md`
3. `docs/architecture.md`

Read these when the task matches:

- `docs/releasing.md`
  - workspace version and tag policy
  - `fmtview-core` before `fmtview` crates.io publishing order
  - changelog and release notes policy
  - GitHub Release artifacts and npm packaging
- `docs/performance.md`
  - viewer, load, syntax, formatter, and diff benchmark smoke tests
  - complete-output formatter algorithm comparison
  - terminal draw byte-count checks
- `docs/visual-verification.md`
  - Xvfb/Kitty/ffmpeg real-terminal recording workflow
  - MP4, frame, keyframe, contact-sheet, and inspection artifacts
- `docs/architecture.md`
  - application versus headless engine crate boundary
  - type profiles, load/index, transform, viewer, and diff ownership
  - backend-neutral input and render frames
  - the public bidirectional record timeline and reset/follow invariants

## Workspace Entry Points

- `Cargo.toml` is both the `fmtview` package manifest and the workspace root.
  The user-facing package and binary name remain `fmtview`.
- `crates/fmtview-core/Cargo.toml` is the publishable headless engine package.
  It is versioned with the root package and has no crossterm dependency.
- `src/bin/fmtview.rs` is the thin binary entry point.
- `src/lib.rs` exposes only application-layer modules used by the binary and
  black-box tests.
- `crates/fmtview-core/src/lib.rs` is the library boundary for profiles,
  loading, viewer/diff engines, backend-neutral input, and render frames.

## Application Package: `src/`

- `src/cli.rs` owns clap types and wires commands to core transforms, loading,
  diff models, and viewer engines. Redirected stdout stays here.
- `src/input.rs` materializes file, stdin, and literal inputs, then hands a
  reopenable `InputSource` to core.
- `src/shell_alias.rs` owns `fmtview alias <bash|zsh|fish>` snippet generation
  and managed startup-file installation.
- `src/viewer.rs` owns crossterm key/mouse/resize translation, terminal polling,
  raw mode, alternate screen, mouse capture, engine-loop scheduling, and cleanup.
- `src/tui/screen.rs` commits a core `RenderFrame` through a ratatui crossterm
  backend and tracks previous buffers for compact redraws.
- `src/tui/scroll.rs` and `src/tui/terminal_writer.rs` own terminal-specific
  scroll-region and ANSI write mechanics.

No application module decides which lines, styles, title, footer, search state,
or navigation target should be displayed. Those decisions belong to core.

## Headless Engine: `crates/fmtview-core/src/`

- `input.rs` defines a reopenable, application-prepared `InputSource`; it does
  not read stdin or inspect whether a process stream is a TTY.
- `profile.rs` and `profile/` resolve explicit types, extensions, and bounded
  sniffing into content shape, load strategy, and transform strategy.
- `formats.rs` and `formats/` own terminal-independent format capabilities:
  transforms, visible-window highlighting, structure candidates, JSON paths,
  chat roles, tool relations, and Markdown fenced-code modes.
- `transform.rs` and `transform/` own whole-source and record transforms used by
  redirected output, eager viewing, lazy record viewing, and diff input.
- `load.rs` and `load/` own `ViewFile`, eager temp-file indexing, lazy record
  spooling, timeline raw/formatted spools, reset reconciliation, read windows,
  progress, notices, and preload mechanics.
- `timeline.rs` defines the public backend-neutral `RecordTimeline` contract,
  identities/snapshots/read/refresh outcomes, and the reverse-scanning growing
  file implementation.
- `diff.rs` and `diff/` own redirected unified patches, eager/lazy interactive
  models, record-stream comparison, inline changes, and unified/side-by-side rows.
- `viewer/input.rs` defines backend-neutral key, pointer, resize, modifier, and
  action types. The root package translates crossterm events into this vocabulary.
- `viewer/file.rs` owns `FileViewer`: background search/structure work, lazy
  preload scheduling, viewer state transitions, frame rendering, and prewarming.
- `viewer/file/` owns search, navigation, sticky breadcrumbs, conversation/tool
  context, Markdown checkpoints, viewport positioning, layout, gutters,
  highlighting, render caches, and tail clamping.
- `viewer/diff.rs` owns `DiffViewer`: preload, layout state, navigation, and
  backend-neutral diff frames.
- `viewer/diff/` owns diff input transitions, visual-row navigation, unified and
  side-by-side render models, wrapped cells, and inline styles.
- `tui/screen.rs` defines `RenderFrame`, `ScrollPosition`, and optional compact
  repaint hints without committing them to a terminal.
- `tui/buffer_frame.rs` paints a `RenderFrame` into an in-memory ratatui buffer.
- `tui/palette.rs`, `tui/text.rs`, and `tui/wrap.rs` own shared styles,
  display-width slicing, wrapping, and checkpoints.
- `perf/` owns generated load/format benchmark cases and the Rust harness used
  by the shell wrappers.

## Tests And Performance

- `crates/fmtview-core/tests/headless_viewer.rs` proves file rendering, search,
  navigation, diff rendering, and buffer painting without a PTY or real terminal.
- `crates/fmtview-core/tests/record_timeline.rs` uses a mutable fake timeline and
  real files to prove pending/end, append/reset, follow detach/reattach, anchor
  preservation, lazy search/navigation, incomplete EOF, and bounded tail work.
- Unit tests under `crates/fmtview-core/src/viewer/tests/` remain close to private
  engine state, render caches, search, viewport, and structure-navigation logic.
- Root `src/tui/tests.rs` covers the actual terminal writer and frame-commit
  adapter, including terminal draw byte-count performance cases.
- `tests/cli.rs` covers public binary contracts and redirected output.
- `benches/load-performance.sh`, `benches/syntax-performance.sh`,
  `benches/viewer-performance.sh`, and `benches/format-performance.sh` are the
  required smoke entry points for the corresponding migrated areas.
- `scripts/record-emulator-demo.sh` runs the built CLI in a real Kitty terminal
  on Xvfb and writes visual artifacts under
  `target/fmtview-emulator-recordings/`.

Keep README user-facing. Keep maintainer-only workflows and durable boundaries
in docs, and link them from `AGENTS.md` when they become required reading.
