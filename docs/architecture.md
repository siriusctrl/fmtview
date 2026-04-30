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
  | DiffInput        |      | plain/jinja   |      | transform plan    |
  +------------------+      +---------------+      | syntax highlighter|
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

`TypeProfile` is the single source of truth for type-specific behavior. It
answers four questions:

- What is the content kind?
- Should the viewer index source text, an eager transformed document, or lazily
  produced transformed lines?
- Which transform should redirected stdout and diff input use?
- Which syntax engine should render visible windows?

## Current Profiles

| Type | Interactive view | Redirected output | Diff input | Syntax |
| --- | --- | --- | --- | --- |
| JSON | Eager transformed document indexed from a temp file | Pretty-printed JSON | Pretty-printed JSON | Structured JSON/XML-style |
| JSONL/NDJSON | Lazy transformed records spooled and indexed on demand | Pretty-printed records | Pretty-printed records; TTY diff can open lazily | Structured JSON/XML-style |
| XML/HTML/XHTML | Eager transformed document indexed from a temp file | Pretty-printed XML-compatible markup | Pretty-printed XML-compatible markup | Structured JSON/XML-style |
| Plain text | Raw source indexed without rewriting content | Passthrough | Passthrough | Plain |
| Jinja | Raw source indexed without rendering or rewriting content | Passthrough | Passthrough | Jinja template spans |

Unknown extensions are sniffed with a bounded prefix. Extensions remain a fast
deterministic hint, but they are not the architecture boundary.

## Load Strategies

The names below describe the behavior we want the code to make explicit:

```text
  EagerIndexedSource
    Read source text as-is, build line offsets, and serve windows from source
    or a passthrough temp file. This is the plain/Jinja shape today.

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

## Strategy Plus Runtime

The same design applies outside loading:

```text
  TypeProfile
      |
      +-- LoadStrategy      -> shared load runtimes
      +-- TransformStrategy -> shared transform engine
      +-- SyntaxKind        -> shared visible-window highlighter
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
3. Keep syntax checkpoint behavior measurable with `benches/syntax-performance.sh`.
4. Explore inline parallelism only after the single-record and checkpointed
   scan baselines are clear.

Inter-line parallel formatting is not the current priority because line and
record streams are already fast enough for common viewer windows. The more
valuable target is huge single logical units: one large JSON record, a large
template section, or a deep visible-window scan. Any future parallelism should
sit behind producer/strategy boundaries so each type can choose whether
parallel work is useful.
