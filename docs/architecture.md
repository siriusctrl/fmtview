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

## Crate Boundary

The workspace has a one-way application-to-engine dependency:

```text
fmtview application package
  clap CLI, stdin/file/literal assembly, redirected stdout
  crossterm event adapter and terminal lifecycle
  raw mode, alternate screen, mouse capture, polling, frame commit, cleanup
                         |
                         v
fmtview-core library package
  format profiles and transforms, load/index models, diff models
  record timeline/source state, formatting/spooling, file/diff viewer state
  viewport anchors, follow state, wrap, search, navigation, highlight
  chat/tool relations, layout and render caches
  backend-neutral InputEvent -> state transition -> RenderFrame
```

`fmtview-core` has no `crossterm` dependency. It can reuse backend-neutral
ratatui types such as `Size`, `Rect`, `Line`, `Style`, and `Buffer`, but it does
not enable ratatui's crossterm feature or create a terminal backend. The root
`fmtview` package maps crossterm key, mouse, and resize events into core
`InputEvent` values. `FileViewer` and `DiffViewer` update engine state and
return a `RenderFrame`; the application owns when that frame is committed and
how terminal state is restored.

This is a product boundary, not only a package boundary. Core tests render
file and diff frames, drive search and navigation, and paint frames into an
in-memory ratatui buffer without raw mode, a PTY, or a terminal backend.

`TypeProfile` selects the format package and shared runtime behavior for the
current input. It answers four questions:

- What is the content kind?
- What input shape does this type expose to the viewer pipeline?
- Should the viewer index source text, an eager transformed document, or lazily
  produced transformed lines?
- Which transform should redirected stdout and diff input use?

Format-specific behavior lives under
`crates/fmtview-core/src/formats/<format>/`. The viewer,
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
| XML | WholeDocument | Eager transformed document indexed from a temp file | Pretty-printed XML-compatible markup | Pretty-printed XML-compatible markup | `formats/xml` |
| HTML | WholeDocument | Eager transformed document indexed from a temp file | Pretty-printed HTML (tolerant tokenizer, text content preserved) | Pretty-printed HTML | `formats/html` (reuses `formats/xml` highlight and structure rules) |
| Markdown | LineIndexed | Raw source indexed without rewriting content | Passthrough | Passthrough | `formats/markdown`, with known fenced code blocks routed to format highlighters |
| TOML | LineIndexed | Raw source indexed without rewriting content | Passthrough | Passthrough | `formats/toml` |
| Plain text | LineIndexed | Raw source indexed without rewriting content | Passthrough | Passthrough | `formats/plain` |
| Jinja | LineIndexed | Raw source indexed without rendering or rewriting content | Passthrough | Passthrough | `formats/jinja` |

Unknown extensions are sniffed with a bounded prefix. Unknown content that does
not look like JSON, JSONL, XML, or HTML falls back to plain-text
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
    - landmark: JSONL record, JSON array item object, chat-style JSON
      message object, heading, table, Jinja block, paragraph start
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

JSON and JSONL also have a small chat-aware structure rule. If an object has a
direct `"role"` field whose value is `"system"`, `"user"`, `"assistant"`, or
`"tool"`, the JSON format package classifies that object as a chat message
candidate.
That can be a top-level array item or a nested object such as
`"message": { "role": "assistant", ... }`. The scanner intentionally checks the
candidate object's direct fields rather than all descendants, so a root object
that merely contains a `messages` array does not absorb every message below it.
The normal JSON/JSONL viewer reuses the same direct-role rule to fill a compact
`S`/`U`/`A`/`T` gutter beside the line-number gutter. The label appears on the
message object's opening line, while a checkpointed container tracker keeps the
role-colored guide active through nested and wrapped interior rows. The guide
is neutral on opening and closing brace rows so adjacent role colors stay
visually separated; sibling objects without a direct role are neutral too. The
same JSON format package owns a bounded tool-link tracker. It only accepts
ID-like fields from contextual tool-call objects or direct `role: tool` result
objects and matches exact values against bounded earlier pending calls. The
viewer consumes those relations as compact endpoint directions in the existing
line-number separator, footer context, and exact `t` navigation. Role and
relation marks are cached in checkpointed windows so adjacent scroll positions
do not repeat the same lookahead scan. Tool-link cold jumps cap recent-prefix
recovery at 4,096 lines, while visible role gutters retain exact scope recovery
from complete checkpoints or the document root. Both checkpoint histories and
per-object ID candidates have hard limits. Open result objects keep a
non-consuming provisional match for body navigation, while closing the object
finalizes the best consistent decision and consumes any matched pending
candidate or candidates. The
directions and role gutter are render-only: they are not part of transformed
output or searchable content. Direction markers add no width; the role gutter
is hidden as width shrinks, and selection mode removes all viewer chrome for
native terminal copying.

JSON key breadcrumbs follow the same boundary. The viewer owns when and where
the breadcrumb is drawn, but JSON path tracking, key parsing, and string escape
handling live in `formats/json`. That prevents viewer features from growing
their own JSON parsers when they need format-aware context.

The implementation mirrors that split:

```text
  crates/fmtview-core/src/viewer/file/structure.rs
    task lifecycle, no-result messages, and ViewState handoff

  crates/fmtview-core/src/viewer/file/structure/scan.rs
    bounded lazy chunk reads and forward/backward scan progress

  crates/fmtview-core/src/viewer/file/structure/candidate.rs
    viewer-side candidate ranking policy

  crates/fmtview-core/src/viewer/file/structure/visibility.rs
    viewport observation rules shared by all formats

  crates/fmtview-core/src/formats/<format>/structure.rs
    JSON, XML/HTML, Markdown, TOML, Jinja, and plain-text structure rules

  crates/fmtview-core/src/formats/<format>/highlight.rs
    visible-window highlighting for each format

  crates/fmtview-core/src/formats/<format>/transform.rs
    formatter implementations for formats that rewrite output
```

## TTY Module Boundaries

The interactive surface is split by responsibility rather than by every file
that happens to draw terminal text:

```text
  src/viewer.rs
    crossterm event translation, raw mode, alternate screen, mouse capture,
    polling, engine-loop scheduling, and cleanup

  src/tui/
    terminal frame commit, buffer delta writes, and scroll-region escape output

  crates/fmtview-core/src/viewer/file.rs
    FileViewer state machine: background work, input transitions, viewport
    positioning, render orchestration, and cache prewarming

  crates/fmtview-core/src/viewer/file/
    search, structure navigation, sticky breadcrumbs, checkpointed
    conversation scopes/tool links, Markdown line modes, viewport models,
    layout, highlighting, and render caches

  crates/fmtview-core/src/viewer/diff.rs
    DiffViewer state machine: diff layout, navigation, preload, and frame output

  crates/fmtview-core/src/viewer/diff/
    diff-specific state transitions, visual-row scrolling, change navigation,
    unified/side-by-side rows, wrapped cells, and inline styling

  crates/fmtview-core/src/diff/
    comparison and diff data: redirected unified patches, eager/lazy model
    selection, record-stream comparison, inline annotation, and row models

  crates/fmtview-core/src/tui/
    backend-neutral palette, styled text slicing, display-width wrapping,
    frame data, and in-memory buffer painting
```

This keeps terminal ownership in the application package and display decisions
in the core package. Normal viewing and diff viewing share primitives where
their models genuinely match: backend-neutral frames, display-width wrapping,
palette/text helpers, and format highlighters. Their row models stay separate
because normal files render indexed document lines, while diffs render unified
or side-by-side comparison rows with change metadata.

## Bidirectional Record Timeline

`fmtview-core::RecordTimeline` is the public source seam for committed record
streams. It is intentionally narrower than a file API:

```rust
pub trait RecordTimeline {
    fn label(&self) -> &str;
    fn snapshot(&self) -> TimelineSnapshot;
    fn load_older(&mut self, limit: RecordLoadLimit) -> Result<TimelineRead>;
    fn load_newer(&mut self, limit: RecordLoadLimit) -> Result<TimelineRead>;
    fn refresh(&mut self) -> Result<TimelineRefresh>;
}
```

The contract has these invariants:

- `label` is stable for the lifetime of the timeline. A snapshot reports the
  source epoch plus committed, observed, and pending boundaries; it is not a
  request to enumerate the source.
- Older and newer cursors are independent. Opening a timeline positions both at
  the current committed tail, `load_older` walks backward in source order, and
  `load_newer` walks forward after refresh. Every request has record and byte
  budgets, though a single record may exceed the byte budget.
- `RecordId { epoch, start_offset, end_offset }` is stable for one committed
  record in one source epoch. A record carries the exact source bytes, including
  its line ending. The source implementation—not the formatter—decides whether
  those bytes form a valid committed record.
- `TimelineRead::Pending` means that a live boundary can yield more records
  later. `End` means that direction is terminal. Refresh separately reports an
  append, no change, pending bytes, terminal end, or a reset caused by
  truncation, replacement, or identity change.
- A reset starts a new identity epoch. The viewer keeps already displayed old
  epoch records as immutable generation history, then appends the replacement
  epoch at that boundary. It reconciles only the bounded, order-preserving
  longest suffix(old)/prefix(new) overlap: identity first and exact raw bytes
  only for reset recovery. It never performs set-based content deduplication,
  so legitimate adjacent duplicate records remain visible. Later older loads
  from the new epoch insert at the epoch boundary and cannot interleave ahead of
  stale old-epoch history.

`FileRecordTimeline` implements the seam for growing newline-delimited files.
It locates committed EOF from bounded reverse chunks, never exposes an
incomplete final line, and lets bounded forward loads do the record work after
a refresh. Refresh validates bounded start/middle/end samples of committed
history independently of file timestamps, so same-identity copytruncate
rewrites outside the old tail are still detected without indexing the whole
file. Inode/device identity is used on Unix; portable fallbacks never treat file
length as identity.

An unchanged incomplete suffix is not reread on every application poll. The
file implementation retains start/middle/end samples for both committed and
pending ranges plus a change stamp for the previously verified newline-free
range. Unix uses nanosecond ctime. On a platform/filesystem where timestamps are
coarse, a same-size rewrite confined to bytes outside every bounded sample may
be detected only after a later observable size, stamp, or sample change; fully
detecting arbitrary in-place rewrites would require reading or indexing the
whole file and would violate tail-first opening. Stat/read races caused by a
concurrent shrink are retried, and snapshot fields are committed only after all
reads succeed.

`RecordTimelineViewFile` owns formatting, on-disk raw/formatted spools, reset
reconciliation, and compact line/record indexes. `FileViewer` owns viewport
anchors, prepend adjustment, search/navigation over lazy boundaries, and the
backend-neutral `Following`/`Detached`/`Paused` state. A future
checkpoint-committed producer can implement the same trait by mapping its own
opaque stable ordering into epoch/offset identities; it needs no file methods,
poll cadence, checkpoint storage rule, or terminal backend type.

The root package only decides when to call core preload/refresh work, maps `f`
and other crossterm events into `InputEvent`/`ViewerCommand`, and commits the
returned frame. Poll cadence, raw mode, terminal cleanup, and TTY detection do
not enter `fmtview-core`.

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
    then view/search/diff that transformed text. This is JSON, XML, and HTML today.

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

The load module is intentionally about one input at a time. Its public surface
is the `ViewFile` contract plus `open_view_file`, which turns the active profile
into either an eagerly indexed temp file or a lazy view file:

```text
  crates/fmtview-core/src/load/open.rs
    InputSource + TypeProfile + FormatOptions
             |
             v
    LazyTransformedRecordsFile
      or
    IndexedTempFile
             |
             v
    Box<dyn ViewFile>

  crates/fmtview-core/src/load/indexed.rs
    eager temp-file line offsets + read_window

  crates/fmtview-core/src/load/lines.rs
    shared line-offset and line-ending helpers
```

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
