# Docs Index

Read in this order when getting oriented:

1. `README.md`
2. `AGENTS.md`

Read these when the task matches:

- `docs/releasing.md`
  - release checklist
  - version and tag policy
  - changelog and release notes policy
  - GitHub Release artifacts
  - crates.io publishing
  - npm wrapper and platform package strategy
- `docs/performance.md`
  - viewer rendering benchmark smoke tests
  - formatter and lazy-preview benchmark smoke tests
  - complete-output formatter algorithm comparison
  - terminal draw byte-count checks
  - comparing performance changes outside CI

Code orientation:

- `src/bin/fmtview.rs` is the thin binary entry point.
- `src/lib.rs` exposes the internal crate modules used by the binary and tests.
- `src/cli.rs` wires CLI arguments to formatting, diff, preview planning, and
  viewer paths.
- `src/format/` owns non-interactive formatting:
  - `engine.rs` orchestrates whole-source and record formatting.
  - `detect.rs` handles format candidates and auto-detection.
  - `json.rs` keeps token-preserving JSON/JSONL formatting.
  - `xml.rs` wraps XML-compatible formatting.
- `src/preview/` owns TTY preview planning and lazy record spooling.
- `src/diff/` owns unified patch generation plus the structured diff model used
  by the interactive diff viewer:
  - `external.rs` formats both sides and shells out to the platform diff tool.
  - `stdout.rs` keeps redirected diff output on the unified patch path.
  - `view.rs` selects the eager or lazy TTY diff model.
  - `lazy_records/` incrementally formats record streams for large TTY diffs.
  - `model/` parses unified patch rows, annotates inline changes, and builds
    the side-by-side row model.
- `src/input.rs` and `src/line_index.rs` own input materialization and
  temp-file indexing.
- `src/viewer/` owns the interactive TUI:
  - `mod.rs` runs the terminal loop and frame composition.
  - `breadcrumb.rs` builds compact sticky JSON key breadcrumbs for the viewer.
  - `diff_view/` handles the interactive single-column and side-by-side diff
    viewer, with input handling and rendering split by responsibility.
  - `terminal.rs` handles terminal diffing, ANSI writes, and scroll regions.
  - `input/` handles key/mouse state, scrolling, jumps, and search.
  - `render/` handles line windows, wrapping, visual rows, caches, progress,
    prewarming, and the search highlight overlay.
  - `highlight/` handles JSON and XML-like syntax highlighting.
  - `palette.rs` owns viewer colors.
  - `tests.rs` keeps viewer regression and performance smoke coverage close to
    the private TUI internals.
- `tests/cli.rs` covers CLI-level behavior.
- `benches/` contains local performance harnesses. They are shell-driven smoke
  checks rather than Cargo benchmark targets because they exercise the release
  binary, ignored perf tests, PTY-like terminal writers, structured diff view
  rendering, and alternate external formatter commands.

Keep README user-facing. Keep maintainer-only workflows in docs and link them
from `AGENTS.md`.
