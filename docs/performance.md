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

Run one shape, layer, or label substring while iterating on load/format work:

```sh
benches/load-performance.sh --samples 3 --case record-stream
benches/format-performance.sh --samples 3 --case huge-record
```

The load and format scripts run a dedicated Rust performance harness in
`src/perf/`. The shell wrappers are compatibility entry points only: Rust owns
fixture generation, samples, case filtering, shape/layer labels, and timing
summaries. `runner.rs` owns timing output, `load.rs` and `format.rs` own case
definitions, and `fixtures.rs` owns generated input data. Viewer, syntax, and
diff scripts still run ignored release-mode tests close to their private
modules; keep migrating those into the Rust performance harness when their
benchmark logic grows beyond a narrow smoke check. Scripts that measure
terminal drawing unset `NO_COLOR` for benchmark subprocesses so byte counts
include the normal styled-color path.

For planned lazy-runtime refactors before inline parallel parser or formatter
work, keep the baseline split across two scripts: `benches/load-performance.sh`
measures lazy first-window and preload behavior, while
`benches/format-performance.sh` separates huge structured records from huge
string records.

Every load/format benchmark prints a `shape` and `layer` line. Use these as the
optimization boundary. The top-level shape names map to `ContentShape`; suffixes
such as `/huge-record` are benchmark stress cases, not separate type-profile
shapes.

- `line-indexed`: source text can be indexed as-is.
- `record-stream`: newline-delimited records can be transformed independently.
- `record-stream/huge-record`: one record dominates the cost, so first-window
  lazy behavior cannot hide transform/readback work.
- `whole-document`: formatting depends on parser state across the complete
  document, so transform, temp-file indexing, and readback should be measured
  separately.
- `transform`, `transform+spool`, `transform+readback`, and related layer names
  separate parser/formatter cost from lazy runtime and viewer readback cost.

Rejected paths should be recorded here when they are likely to be tempting
again. Indexing transformed lines while writing the temp file was tested for
whole-document output, but the benchmark did not justify the added writer
complexity; keep temp-file indexing separate until new data shows a stable win.

Metrics:

Load metrics:

- `raw indexed load` measures building line offsets for a large already-textual
  input and reading a middle window from the index. Shape: `line-indexed`.
- `lazy record first-window load+transform` measures opening a lazy record stream,
  transforming only enough records to fill the first visible window, and
  reading those spooled lines back. Shape: `record-stream`.
- `lazy record preload load+transform` measures background lazy record
  transform plus spool/index extension after the first window has opened.
  Shape: `record-stream`.
- `lazy huge string first-window load+transform` measures a single JSONL record
  where most bytes are inside one large string value. This keeps the lazy
  record first-window path honest for files that cannot benefit from reading a
  few small records. Shape: `record-stream/huge-record`.
- `lazy huge string preload transform+spool` measures the same shape without
  reading the visible window back from the spool. Compare this with the
  first-window metric to separate transform/spool cost from long-line readback.
  Shape: `record-stream/huge-record`.
- `json whole-document eager view open` and `xml whole-document eager view open`
  measure complete document transform, temp-file line indexing, and first-window
  readback together. Shape: `whole-document`.
- `json whole-document index+readback` and `xml whole-document index+readback`
  measure the post-transform viewer-open cost for already formatted document
  temps. Shape: `whole-document`.

Transform metrics:

- `jsonl record batch CPU` measures formatting many independent JSONL records
  from memory. This is the target for inter-line record parallelism without
  temp-file write noise. Shape: `record-stream`.
- `jsonl source full format` measures full JSONL file formatting through the
  normal temp-file path, including read, parse, format, and write. Shape:
  `record-stream`.
- `single huge object-array record format` measures one large JSON record with
  many object children inside an array. This is the target shape for future
  structural split plus ordered inline-parallel formatting. Shape:
  `record-stream/huge-record`.
- `single huge string field record format` measures one large JSON object where
  most bytes are inside a single string value. This is the target shape for
  string scan/copy improvements; structural child parallelism is not expected
  to help much here. Shape: `record-stream/huge-record`.
- `json whole-document format` measures complete JSON document pretty-printing
  without the following viewer index pass. Shape: `whole-document`.
- `xml whole-document format` measures complete XML-compatible document
  pretty-printing without the following viewer index pass. Shape:
  `whole-document`.

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
