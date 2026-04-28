# Performance Checks

Use this when changing viewer rendering, syntax highlighting, wrapping,
terminal drawing, parsing, formatting, JSONL record handling, or lazy preview
behavior.

Run the viewer benchmark smoke suite for TUI rendering and terminal bytes:

```sh
scripts/bench-viewer-performance.sh
```

Run the formatter benchmark smoke suite for parser, record-stream, whole-record,
and lazy-preview work:

```sh
scripts/bench-format-performance.sh
```

Run the algorithm comparison harness when testing an alternate complete-output
formatter implementation:

```sh
scripts/bench-format-algorithm.sh --candidate 'experiment=target/release/fmtview --type {type} --indent {indent} {input}'
```

Use fewer samples while iterating:

```sh
scripts/bench-viewer-performance.sh --samples 3
scripts/bench-format-performance.sh --samples 3
scripts/bench-format-algorithm.sh --samples 3 --candidate 'experiment=...'
```

The script runs ignored release-mode tests, so normal `cargo test` and CI stay
focused on correctness. It unsets `NO_COLOR` for the benchmark subprocesses so
the terminal draw byte count includes the normal styled-color path.

Metrics:

Formatter metrics:

- `jsonl record batch CPU` measures formatting many independent JSONL records
  from memory. This is the target for inter-line record parallelism without
  temp-file write noise.
- `jsonl source full format` measures full JSONL file formatting through the
  normal temp-file path, including read, parse, format, and write.
- `single huge record format` measures one very large JSON record. This is the
  target for future inline chunk/checkpoint parser work.
- `lazy first window format` measures opening a lazy JSONL preview and
  formatting enough records for the first window.
- `lazy preload records format` measures background lazy record formatting,
  which is the target for future worker-pool preloading.

Algorithm comparison:

- `scripts/bench-format-algorithm.sh` generates fixed JSONL and JSON fixtures,
  formats each fixture with the current release `fmtview` as the reference, then
  runs one or more candidate commands.
- Candidate output must match the reference byte-for-byte. The script reports
  `aligned=yes` per sample and fails immediately on mismatch.
- Candidate commands use `{input}`, `{output}`, `{type}`, and `{indent}`
  placeholders. If `{output}` is omitted, stdout is redirected to the sample
  output path.
- This harness is meant for comparing parser/formatter algorithms without first
  wiring each experiment into production code. It measures complete formatted
  output, not lazy first-window behavior.

Viewer metrics:

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
