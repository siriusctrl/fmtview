# Performance Checks

Use this when changing viewer rendering, syntax highlighting, wrapping,
terminal drawing, parsing, formatting, JSONL record handling, or lazy loading
behavior.

The harnesses are split by layer. Run the narrow layer first while iterating,
then run the broader viewer or diff checks when the user-visible terminal
surface may change.

Run the viewer benchmark smoke suite for TUI rendering and terminal bytes:

```sh
benches/viewer-performance.sh
```

Run the load benchmark smoke suite for raw indexed files and lazy record
spooling:

```sh
benches/load-performance.sh
```

Run the syntax benchmark smoke suite for visible-window highlighter work:

```sh
benches/syntax-performance.sh
```

Run the diff benchmark smoke suite for structured diff model building and
interactive diff-view rendering:

```sh
benches/diff-performance.sh
```

Run the transform benchmark smoke suite for parser, record-stream, and
whole-record formatter work:

```sh
benches/format-performance.sh
```

Run the algorithm comparison harness when testing an alternate complete-output
formatter implementation:

```sh
benches/format-algorithm.sh --candidate 'experiment=target/release/fmtview --type {type} --indent {indent} {input}'
```

Use fewer samples while iterating:

```sh
benches/viewer-performance.sh --samples 3
benches/load-performance.sh --samples 3
benches/syntax-performance.sh --samples 3
benches/diff-performance.sh --samples 3
benches/format-performance.sh --samples 3
benches/format-algorithm.sh --samples 3 --candidate 'experiment=...'
```

The script runs ignored release-mode tests, so normal `cargo test` and CI stay
focused on correctness. It unsets `NO_COLOR` for the benchmark subprocesses so
the terminal draw byte count includes the normal styled-color path.

Metrics:

Load metrics:

- `raw indexed load` measures building line offsets for a large already-textual
  input and reading a middle window from the index.
- `lazy first window load+transform` measures opening a lazy record stream,
  transforming only enough records to fill the first visible window, and
  reading those spooled lines back.
- `lazy preload records load+transform` measures background lazy record
  transform plus spool/index extension after the first window has opened.

Transform metrics:

- `jsonl record batch CPU` measures formatting many independent JSONL records
  from memory. This is the target for inter-line record parallelism without
  temp-file write noise.
- `jsonl source full format` measures full JSONL file formatting through the
  normal temp-file path, including read, parse, format, and write.
- `single huge record format` measures one very large JSON record. This is the
  target for future inline chunk/checkpoint parser work.

Syntax metrics:

- `syntax highlight window` measures visible-window span generation across a
  huge logical line with checkpoint reuse. It excludes file IO, formatting,
  terminal drawing, and wrap computation.

Algorithm comparison:

- `benches/format-algorithm.sh` generates fixed JSONL and JSON fixtures,
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
  terminal backend writes anything. It includes wrapping and syntax span use,
  but excludes terminal IO.
- `terminal draw bytes` measures repeated viewer drawing into a counting
  terminal writer, including terminal bytes and background-cell count.
- `terminal visual-row scroll bytes` measures repeated scrolling inside one
  extremely long wrapped logical line, which is the path most likely to expose
  visible terminal repaint artifacts.
- `background_cells` should move toward zero for normal non-search scrolling.
  Search highlighting may still use background color for match spans.

Diff metrics:

- `diff model build` measures parsing unified patch text into the structured
  row model used by the TTY diff viewer, including shared row storage,
  side-by-side indexes, changed-row indexes, and eager inline-diff budgeting.
- `lazy record diff view open` measures the TTY diff open path for two
  different large record-stream inputs, including the first bounded lazy scan.
- `interactive diff view render` measures repeated visible-window rendering for
  both single-column and side-by-side diff layouts without terminal write noise.

When comparing changes, run the script on both commits with the same
`--samples` value and compare the median numbers.
