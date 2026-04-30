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
- `docs/architecture.md`
  - viewer-first product model
  - type profile boundaries
  - load, transform, syntax, and diff strategy boundaries
  - lazy runtime and producer design
  - benchmark-first direction for future inline parallel work

Code orientation:

- `src/bin/fmtview.rs` is the thin binary entry point.
- `src/lib.rs` exposes the internal crate modules used by the binary and tests.
- `src/cli.rs` wires CLI arguments to transforms, diff, load planning, and
  viewer paths.
- `src/input.rs` owns input materialization from files, stdin, and literals.
- `src/profile.rs` resolves `--type` and auto-detection into a concrete content
  kind, input shape, load strategy, transform strategy, and syntax highlighter.
- `src/load/` owns lazy-load runtimes and indexed line access:
  - `mod.rs` defines `ViewFile` plus eager temp-file line indexing.
  - lazy loaders share spool/index/view-window mechanics and keep
    format-specific record or document reading behind producer boundaries.
- `src/transform/` owns content transforms that can produce scriptable output:
  - `engine.rs` orchestrates whole-source and record formatting.
  - `detect.rs` handles formatter candidates after profile resolution.
  - `json.rs` keeps token-preserving JSON/JSONL formatting.
  - `xml.rs` wraps XML-compatible formatting.
- `src/syntax/` owns visible-window syntax highlighting and checkpoint state.
  It must not read input files or transform content.
- `src/diff/` owns unified patch generation plus the structured diff model used
  by the interactive diff viewer:
  - `external.rs` formats both sides and shells out to the platform diff tool.
  - `stdout.rs` keeps redirected diff output on the unified patch path.
  - `view.rs` selects the eager or lazy TTY diff model.
  - `lazy_records/` incrementally formats record streams for large TTY diffs.
  - `model/` parses unified patch rows, annotates inline changes, and builds
    the side-by-side row model.
- `src/viewer/` owns the interactive TUI:
  - `mod.rs` runs the terminal loop and frame composition.
  - `breadcrumb.rs` builds compact sticky JSON key breadcrumbs for the viewer.
  - `diff_view/` handles the interactive single-column and side-by-side diff
    viewer, with input handling and rendering split by responsibility.
  - `terminal.rs` handles terminal diffing, ANSI writes, and scroll regions.
  - `input/` handles key/mouse state, scrolling, jumps, and search.
  - `render/` handles line windows, wrapping, visual rows, caches, progress,
    prewarming, and the search highlight overlay.
  - `palette.rs` owns viewer colors.
  - `tests.rs` keeps viewer regression and performance smoke coverage close to
    the private TUI internals.
- `tests/cli.rs` covers CLI-level behavior.
- `src/perf/` owns the Rust load/format performance harness used by the shell
  wrappers:
  - `runner.rs` owns sample counts, filtering, timing summaries, and output.
  - `load.rs` and `format.rs` own benchmark case definitions.
  - `fixtures.rs` owns generated benchmark input data.
- `benches/` contains local performance entry points:
  - load and format wrappers call the Rust harness in `src/perf/`.
  - syntax, viewer, diff, and algorithm checks still use focused shell-driven
    smoke harnesses because they exercise private TUI internals, terminal
    writers, release binaries, or alternate external formatter paths.

Keep README user-facing. Keep maintainer-only workflows in docs and link them
from `AGENTS.md`.
