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
  - format packages plus load, transform, viewer, and diff runtime boundaries
  - lazy runtime and producer design
  - benchmark-first direction for future inline parallel work

Code orientation:

- `src/bin/fmtview.rs` is the thin binary entry point.
- `src/lib.rs` exposes the internal crate modules used by the binary and tests.
- `src/cli.rs` wires CLI arguments to transforms, diff, load planning, and
  viewer paths.
- `src/input.rs` owns input materialization from files, stdin, and literals.
- `src/profile.rs` resolves `--type` and auto-detection into a concrete content
  kind, input shape, load strategy, and transform strategy.
- `src/formats.rs` owns the format-capability entry point. Each folder under
  `src/formats/` groups one format's behavior:
  - `json/` owns JSON formatting, highlighting, chat-role detection, and
    structure-jump rules.
  - `jsonl/` owns the JSONL profile while reusing JSON record behavior.
  - `xml/` owns XML-compatible formatting, highlighting, and structure rules.
  - `markdown/` owns Markdown highlighting, fenced-code line modes, and
    heading structure rules.
  - `toml/`, `jinja/`, and `plain/` own their respective highlighting and
    structure rules.
  - `shared.rs` keeps small helpers reused by multiple format packages.
- `src/load.rs` owns the load-module entry point and exports the load-facing
  API.
- `src/load/` owns single-input view-file construction, indexing, lazy-load
  runtimes, and load planning helpers:
  - `view_file.rs` defines the `ViewFile` contract consumed by viewer code.
  - `open.rs` converts a resolved `TypeProfile` into the right `ViewFile`.
  - `indexed.rs` owns eager temp-file line indexing and read-window access.
  - `lines.rs` owns shared line-offset and line-ending helpers.
  - lazy loaders share spool/index/view-window mechanics and keep
    format-specific record or document reading behind producer boundaries.
  - `record_stream.rs` owns newline-delimited record access shared by lazy
    viewing and lazy diffing: raw record reads, source offsets, per-record
    formatting, lookahead windows, and unread buffers.
- `src/transform.rs` owns the transform-module entry point.
- `src/transform/` owns content transforms that can produce scriptable output:
  - `engine.rs` orchestrates whole-source and record formatting.
  - `detect.rs` handles formatter candidates after profile resolution.
  - Format-specific formatter implementations live under `src/formats/<type>/`.
- `src/diff.rs` owns the diff-module entry point.
- `src/diff/` owns unified patch generation plus the structured diff model used
  by redirected output and the interactive diff viewer:
  - `external.rs` formats both sides and shells out to the platform diff tool.
  - `stdout.rs` keeps redirected diff output on the unified patch path.
  - `view.rs` selects the eager or lazy TTY diff model.
  - `record_stream.rs` incrementally formats record streams for large TTY diffs.
    It consumes `load::record_stream` readers and owns only two-sided
    comparison, resynchronization, context omission, and diff row generation.
  - `model/` parses unified patch rows, annotates inline changes, and builds
    the side-by-side row model.
- `src/tui.rs` owns shared terminal UI primitives that are not specific to the
  normal file viewer or diff viewer:
  - `screen.rs` handles terminal buffer rendering, ANSI writes, scroll regions,
    and buffer-delta repainting.
  - `palette.rs` owns terminal colors used by format highlighting, viewer, and diff output.
  - `text.rs` and `wrap.rs` own shared character-counting, styled text slicing,
    display-width wrapping, and wrap checkpoint helpers.
- `src/viewer.rs` owns the shared TTY lifecycle: raw mode, alternate screen,
  mouse capture setup, cleanup, and dispatch to file or diff viewer loops.
- `src/viewer/` owns viewer submodules:
  - `file.rs` and `file/` own the normal file viewer mode for indexed/lazy file
    windows.
    - `file/input.rs` and `file/input/` handle key/mouse state, scrolling, line
      jumps, and search.
    - `file/render.rs` and `file/render/` handle line windows, visual rows,
      layout/sticky coordination, title/footer text, viewer gutter layout,
      caches, progress, prewarming, and the search highlight overlay. Shared
      text wrapping lives in `src/tui/`.
    - `file/structure.rs` and `file/structure/` own the `]`/`[` smart structure
      jump: task state, lazy scans, candidate ranking, and viewport visibility.
      Format-specific structure rules live under
      `src/formats/<type>/structure.rs`.
    - `file/breadcrumb.rs` builds compact sticky JSON key breadcrumbs.
    - `file/markdown_modes.rs` owns viewer-time Markdown fenced-code
      checkpointing. The per-line mode rules live with Markdown under
      `src/formats/markdown/`.
    - `file/position.rs` resolves search/structure targets to visible viewport
      positions and clamps tail positions.
  - `diff.rs` and `diff/` own the interactive diff viewer mode. They consume
    `src/diff/` models but keep terminal-facing behavior in the viewer layer.
    - `diff/input.rs` handles diff-viewer keys and mouse events.
    - `diff/navigation.rs` handles change-block jumps and visual-row scrolling.
    - `diff/render.rs` and `diff/render/` own all diff TTY rows, title/footer
      text, unified and side-by-side layout, and inline diff styling.
  - `tests/` keeps white-box viewer regression and performance smoke coverage
    close to private TUI internals, split by input, navigation, search, render,
    screen, cache, highlighting, and viewport responsibility. These tests stay under
    `src/` because they intentionally exercise private render caches,
    terminal-buffer reuse, viewer state, and jump helpers without widening the
    public API. Structure-navigation tests live under `tests/structure/` and
    mirror the implementation split: detection, format behavior, interaction
    state, JSON ranking/visibility, and target clamping.
- `tests/cli.rs` covers black-box CLI behavior through the compiled binary and
  should be the home for public command/output contracts.
- `src/perf/` owns the Rust load/format performance harness used by the shell
  wrappers:
  - `runner.rs` owns sample counts, filtering, timing summaries, and output.
  - `load.rs` and `format.rs` own benchmark case definitions.
  - `fixtures.rs` owns generated benchmark input data.
- `benches/` contains local performance entry points:
  - load and format wrappers call the Rust harness in `src/perf/`.
  - highlighter, viewer, diff, and algorithm checks still use focused shell-driven
    smoke harnesses because they exercise private TUI internals, terminal
    writers, release binaries, or alternate external formatter paths.

Keep README user-facing. Keep maintainer-only workflows in docs and link them
from `AGENTS.md`.
