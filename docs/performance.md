# Performance Checks

Use this when changing viewer rendering, syntax highlighting, wrapping, or
terminal drawing behavior.

Run the viewer benchmark smoke suite:

```sh
scripts/bench-viewer-performance.sh
```

Use fewer samples while iterating:

```sh
scripts/bench-viewer-performance.sh --samples 3
```

The script runs ignored release-mode tests, so normal `cargo test` and CI stay
focused on correctness. It unsets `NO_COLOR` for the benchmark subprocesses so
the terminal draw byte count includes the normal styled-color path.

Metrics:

- `viewport render CPU` measures repeated wrapped viewport rendering before the
  terminal backend writes anything.
- `terminal draw bytes` measures repeated viewer drawing into a counting
  terminal writer, including terminal bytes and background-cell count.
- `terminal visual-row scroll bytes` measures repeated scrolling inside one
  extremely long wrapped logical line, which is the path most likely to expose
  visible terminal repaint artifacts.
- `background_cells` should move toward zero for normal non-search scrolling.
  Search highlighting may still use background color for match spans.

When comparing changes, run the script on both commits with the same
`--samples` value and compare the median numbers.
