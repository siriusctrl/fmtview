# Architecture

`fmtview` is a CLI viewer first. Formatting is one way to prepare text for the
viewer, redirected stdout, and diff input, but the product surface is broader:
highlight, view, search, and diff large files without giving up scriptable
terminal behavior.

The core boundary is:

```text
  +------------------+      +---------------+      +-------------------+
  | Use case         |      | Input type    |      | TypeProfile       |
  |                  |      |               |      |                   |
  | InteractiveView  +----->+ json/jsonl    +----->+ content kind      |
  | RedirectedOutput |      | xml/html      |      | load strategy     |
  | DiffInput        |      | md/toml/text  |      | transform plan    |
  +------------------+      +---------------+      | input shape       |
                                                   | format behavior   |
                                                   +---------+---------+
                                                             |
                                                             v
                         +----------------+------------------+----------------+
                         |                |                  |                |
                         v                v                  v                v
                  +-------------+  +---------------+  +---------------+  +-----------+
                  | Load/index  |  | Transform     |  | Highlight     |  | Diff/view |
                  +-------------+  +---------------+  +---------------+  +-----------+
```

`TypeProfile` selects the format package and shared runtime behavior for the
current input. It answers four questions:

- What is the content kind?
- What input shape does this type expose to the viewer pipeline?
- Should the viewer index source text, an eager transformed document, or lazily
  produced transformed lines?
- Which transform should redirected stdout and diff input use?

Format-specific behavior lives under `src/formats/<format>/`. The viewer,
loader, transformer, and diff code remain runtime layers: they ask the active
format package how to highlight visible text, format records/documents, resolve
Markdown fenced-code line modes, or classify structure-jump candidates.

## Input Shapes

`ContentShape` is the coarse performance and capability boundary. It is not a
parser interface and it does not decide highlighting by itself. It names the
unit of work that shared runtimes can rely on:

```text
  LineIndexed
    Source text already has usable line boundaries and does not need a
    formatter before viewing. Markdown, TOML, plain text, and Jinja use this
    shape today.

  RecordStream
    Input is a sequence of independent newline-delimited records. The first
    viewer window can transform only the records it needs, preload can advance
    in bounded record batches, and future ordered parallel transform can share
    the same runtime. JSONL and NDJSON use this shape today.

  WholeDocument
    Correct formatting depends on document-level parser state. The transformed
    document normally has to be produced before the viewer can index it. JSON,
    XML, and HTML use this shape today.
```

Optimizations should say which shape they target. Record-stream work such as
lazy preload and ordered record parallelism should not leak into whole-document
code paths. Whole-document work should focus on streaming parser/formatter
behavior, temp-file indexing, highlight checkpoints, and viewer readback.

## Current Profiles

| Type | Shape | Interactive view | Redirected output | Diff input | Format package |
| --- | --- | --- | --- | --- | --- |
| JSON | WholeDocument | Eager transformed document indexed from a temp file | Pretty-printed JSON | Pretty-printed JSON | `formats/json` |
| JSONL/NDJSON | RecordStream | Lazy transformed records spooled and indexed on demand | Pretty-printed records | Pretty-printed records; TTY diff can open lazily | `formats/jsonl` profile plus JSON record behavior |
| XML/HTML/XHTML | WholeDocument | Eager transformed document indexed from a temp file | Pretty-printed XML-compatible markup | Pretty-printed XML-compatible markup | `formats/xml` |
| Markdown | LineIndexed | Raw source indexed without rewriting content | Passthrough | Passthrough | `formats/markdown`, with known fenced code blocks routed to format highlighters |
| TOML | LineIndexed | Raw source indexed without rewriting content | Passthrough | Passthrough | `formats/toml` |
| Plain text | LineIndexed | Raw source indexed without rewriting content | Passthrough | Passthrough | `formats/plain` |
| Jinja | LineIndexed | Raw source indexed without rendering or rewriting content | Passthrough | Passthrough | `formats/jinja` |

Unknown extensions are sniffed with a bounded prefix. Unknown content that does
not look like JSON, JSONL, or XML-compatible markup falls back to plain-text
passthrough. Extensions remain a fast deterministic hint, but they are not the
architecture boundary.

## Ranked Structure Navigation

Viewer `]`/`[` navigation is a ranked structural jump. It is meant to complement
line-number jumps and search: line jumps are exact addresses, search jumps are
text addresses, and structure jumps move through the document outline. The
scanner remains lazy and line-window based, but it no longer treats visibility
as the whole rule.

```text
  read a bounded line chunk
          |
          v
  classify candidate structure points
    - landmark: JSONL record, JSON array item object, heading, table,
      Jinja block, paragraph start
    - detail: JSON object/array field, XML/HTML start tag
          |
          v
  infer block extent for the active format
          |
          v
  combine kind + viewport visibility
    - visible landmarks can still be selected
    - small fully visible detail blocks are skipped
    - partially visible, wrapped, clipped, or off-screen blocks stay eligible
          |
          v
  publish a viewer target
```

The block extent rule is format-specific. JSON uses string-aware bracket
pairing, Markdown uses heading levels, TOML uses the next table header, Jinja
uses matching block/end tags, XML/HTML uses start and end tags where possible,
and plain text uses paragraph boundaries. The shared visibility rule records
whether the candidate starts above the current viewport, ends below it, is cut
by a wrapped top or bottom row, or is horizontally clipped in nowrap mode.

This keeps `]`/`[` stable enough for large JSON lists while preserving the
useful smart behavior for long inline values and blocks that are not fully
observable on the current screen. Format-specific code decides what counts as a
candidate; shared viewer code still owns visibility, ranking, and scroll
clamping.

The implementation mirrors that split:

```text
  viewer/structure.rs
    task lifecycle, no-result messages, and ViewState handoff

  viewer/structure/scan.rs
    bounded lazy chunk reads and forward/backward scan progress

  viewer/structure/candidate.rs
    viewer-side candidate ranking policy

  viewer/structure/visibility.rs
    viewport observation rules shared by all formats

  formats/<format>/structure.rs
    JSON, XML/HTML, Markdown, TOML, Jinja, and plain-text structure rules

  formats/<format>/highlight.rs
    visible-window highlighting for each format

  formats/<format>/transform.rs
    formatter implementations for formats that rewrite output
```

## TTY Module Boundaries

The interactive surface is split by responsibility rather than by every file
that happens to draw terminal text:

```text
  viewer.rs
    raw mode, alternate screen, mouse capture, and dispatch

  tui/
    reusable terminal primitives: color palette, screen repainting, gutter text,
    display-width wrapping, and wrap checkpoints

  viewer/
    normal file viewer: file loop, input/search, structure navigation, sticky
    breadcrumbs, render caches, viewport positioning, and Markdown line modes

  diff/viewer/
    interactive diff viewer: diff-specific input, change-block navigation,
    unified/side-by-side rendering, and lazy diff model preloading
```

This keeps the normal viewer from owning diff behavior, keeps diff rendering
near the diff model, and prevents low-level terminal repaint or text wrapping
from becoming file-viewer-specific code.

## Load Plans

The names below describe the behavior we want the code to make explicit:

```text
  EagerIndexedSource
    Read source text as-is, build line offsets, and serve windows from source
    or a passthrough temp file. This is the Markdown/TOML/plain/Jinja shape
    today.

  LazyIndexedSource
    Future source-indexed variant for very large raw files where even first
    indexing should be incremental.

  EagerTransformedDocument
    Transform the complete source into a temp file, index transformed lines,
    then view/search/diff that transformed text. This is JSON and XML today.

  LazyTransformedDocument
    Future whole-document transform that can emit transformed lines
    incrementally. This is the likely direction for template-like or structured
    document formats where the transform is not naturally record-delimited.

  LazyTransformedRecords
    Read one logical record at a time, transform that record, append produced
    lines to a spool file, and extend line indexes on demand. This is JSONL
    today.
```

`EagerTransformedDocument` can use a lot of memory or CPU before the first draw
because the transformed document must exist before indexing starts.
`LazyTransformedRecords` trades complete upfront knowledge for fast first draw
and bounded visible-window work. Its transformed text is still written, but it
is written to an on-disk temporary spool instead of retained as a full in-memory
string.

## Shared Lazy Runtime

Lazy loading should share the file/index/spool/viewer runtime and isolate the
type-specific producer. The intended shape is:

```text
                         +-------------------------------+
                         | LazyFile<P: LazyProducer>     |
                         |                               |
                         | - ViewFile implementation     |
                         | - line offset index           |
                         | - source byte progress        |
                         | - temp spool management       |
                         | - read_window / preload loop  |
                         +---------------+---------------+
                                         |
                 +-----------------------+-----------------------+
                 |                       |                       |
                 v                       v                       v
      +----------------------+  +----------------------+  +----------------------+
      | SourceLineProducer   |  | RecordTransform      |  | DocumentTransform    |
      |                      |  | Producer             |  | Producer             |
      | raw line -> line     |  | raw record -> lines  |  | source chunk -> lines|
      |                      |  |                      |  | future              |
      +----------------------+  +----------------------+  +----------------------+
```

The runtime owns mechanics that should not be duplicated across formats:

- when a window needs more lines;
- how many producer steps a preload pass may run;
- how transformed lines are appended to a temporary spool;
- how viewer line numbers map back to source byte offsets;
- how progress and exact/inexact line count are reported.

The producer owns the format-specific state machine:

- how to read the next logical unit;
- how to transform or pass that unit through;
- which source byte offset the produced lines represent;
- when the input is complete.

This keeps future format support from becoming a set of unrelated lazy viewers.
Adding a new lazy format should usually mean adding a producer, not another
`ViewFile` implementation.

## Record Stream Access And Diff

Newline-delimited record access belongs to `load`, even when the consumer is a
diff view. The shared layer is `load::record_stream`:

```text
  load::record_stream
    RawRecordReader
      - read one newline-delimited source record
      - track source byte offsets and byte counts
      - reuse the raw input buffer

    FormattedRecordReader
      - format one record with TransformStrategy-compatible options
      - expose formatted bytes for lazy viewer spooling
      - expose formatted lines plus lookahead/unread for lazy diffing
```

Consumers stay separate:

```text
  load::lazy_records
    formatted record bytes -> LazyBatch -> LazyFile spool/index -> ViewFile

  diff::record_stream
    left/right formatted record lines -> resync/context rules -> DiffModel
```

This avoids duplicate record readers while keeping diff comparison out of
`load`. Future record-batch or ordered-parallel formatting should start at the
shared record-stream access layer, then let each consumer decide how to use the
formatted records.

## Strategy Plus Runtime

The same design applies outside loading:

```text
  TypeProfile
      |
      +-- ContentShape      -> optimization and capability boundary
      +-- LoadPlan          -> shared load runtimes
      +-- TransformStrategy -> shared transform engine
      +-- Format package    -> highlight, structure, and formatter behavior
      +-- Diff mode         -> shared eager/lazy diff runtimes
```

Strategies decide what should happen for a type and use case. Runtimes execute
the common mechanics. Format-specific code should live at the producer/parser
edge instead of leaking into CLI branching, viewer rendering, or diff rendering.

## Performance Direction

Large-file behavior is a product requirement, so performance work should stay
benchmark-backed. The current priority order is:

1. Preserve scriptable stdout and fast first draw.
2. Keep lazy record loading and deep-window viewing measurable with
   `benches/load-performance.sh`.
3. Keep highlight checkpoint behavior measurable with `benches/syntax-performance.sh`.
4. Explore inline parallelism only after the single-record and checkpointed
   scan baselines are clear.

Inter-line parallel formatting is not the current priority because line and
record streams are already fast enough for common viewer windows. The more
valuable target is huge single logical units: one large JSON record, a large
template section, or a deep visible-window scan. Any future parallelism should
sit behind producer/strategy boundaries so each type can choose whether
parallel work is useful.
