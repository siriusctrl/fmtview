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

- `src/main.rs` wires CLI arguments to formatting, diff, and viewer paths.
- `src/format.rs`, `src/diff.rs`, `src/input.rs`, `src/lazy.rs`, and
  `src/line_index.rs` own non-interactive formatting, input materialization,
  lazy preview indexing, and temp-file indexing.
- `src/viewer/` owns the interactive TUI:
  - `mod.rs` runs the terminal loop and frame composition.
  - `input.rs` handles key/mouse state, scrolling, jumps, and search.
  - `render.rs` handles line windows, wrapping, visual rows, caches, progress
    calculation, and the search highlight overlay.
  - `highlight.rs` handles JSON, diff, and XML-like syntax highlighting.
  - `palette.rs` owns viewer colors.
  - `tests.rs` keeps viewer regression and performance smoke coverage close to
    the private TUI internals.
- `tests/cli.rs` covers CLI-level behavior.

Keep README user-facing. Keep maintainer-only workflows in docs and link them
from `AGENTS.md`.
