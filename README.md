# fmtview

Fast CLI viewing, highlighting, search, and diffing for JSON, JSONL,
XML-compatible markup, plain text, and Jinja templates.

`fmtview` is built for the workflow where you want to inspect data quickly in a
terminal: open large files without waiting for a full render, keep syntax
highlighting useful, search the visible text, and diff formatted or passthrough
inputs without leaving the CLI.

```sh
fmtview payload.json
fmtview events.jsonl
fmtview response.xml
fmtview page.html
fmtview template.html.j2
fmtview app.log
fmtview diff old.json new.json
```

If stdout is a terminal, `fmtview` opens an interactive viewer. If stdout is
redirected, it stays scriptable and writes transformed text or diff output:

```sh
fmtview payload.json > pretty.json
fmtview diff old.json new.json > changes.diff
```

## Why

Pretty-printers are useful, but they usually dump text and hand scrolling to
your pager. Pagers are useful, but they do not understand structured data,
embedded markup, wrapped records, or formatted diffs.

`fmtview` combines the two:

- View files in a terminal UI with line numbers, progress, and indent-aware
  soft wrap.
- Highlight JSON, XML-compatible markup, embedded markup in JSON strings, and
  Jinja templates.
- Search the visible text without loading rendered output into memory.
- Diff inputs after applying each input's profile, with interactive
  single-column and side-by-side layouts in a TTY and unified patches on
  redirected stdout.
- Format JSON, JSONL, and XML-compatible markup when that is the right content
  strategy.
- Preview plain text and Jinja templates without rewriting their content.
- Scroll with the keyboard, mouse wheel, or a trackpad without re-rendering on
  every individual input event.
- Highlight JSON string escape tokens such as `\n`, `\t`, `\r`, `\"`, and
  `\\`.
- Pair XML-style opening and closing tags by depth, including markup embedded
  inside JSON string values.
- Keep a compact sticky JSON key breadcrumb above the viewer body while
  scrolling nested documents.
- Preserve data semantics. JSON strings are highlighted for readability, not
  rewritten.
- Keep large outputs responsive by indexing a temporary text file and only
  reading the visible window.

## Install

### Cargo

Install from crates.io:

```sh
cargo install fmtview --locked
```

### npm

Install the prebuilt static Linux x64 binary from npm:

```sh
npm install -g fmtview
```

Use Cargo or the GitHub Release artifacts for other platforms.

See `CHANGELOG.md` for release notes.

### Git

Install directly from the repository:

```sh
cargo install --git https://github.com/siriusctrl/fmtview --locked
```

For local development:

```sh
git clone https://github.com/siriusctrl/fmtview
cd fmtview
cargo test
cargo build --release
```

## Quick Start

Preview a file:

```sh
fmtview data.json
```

Read from stdin:

```sh
curl -s https://example.com/payload.json | fmtview --type json
```

## Type Detection

`fmtview` resolves every input to a type profile: content kind, load strategy,
transform strategy, and syntax highlighter. File extensions are only one signal.
When the extension is unknown, `fmtview` sniffs a bounded prefix of the content:
JSON-looking documents use the JSON formatter, record streams use the lazy JSONL
path, markup-looking documents use the XML-compatible formatter, and otherwise
the input falls back to the record formatter for compatibility with earlier
auto-detection behavior.

Known extensions still provide a fast, deterministic hint:

- `.json` -> JSON formatting.
- `.jsonl` and `.ndjson` -> lazy JSONL record formatting.
- `.xml`, `.html`, `.htm`, and `.xhtml` -> XML-compatible markup formatting.
- `.txt`, `.text`, and `.log` -> plain-text passthrough.
- `.j2`, `.jinja`, and `.jinja2`, including names such as `.html.j2`, ->
  Jinja-template passthrough.

Use `--type` when stdin or an unusual extension needs an explicit profile.

## Formatting Behavior

`fmtview` still formats the structured types:

- JSON documents are pretty-printed with stable indentation while preserving
  number tokens and string values.
- JSONL and NDJSON inputs are processed record by record. Each record is
  formatted as JSON, and large record streams can open lazily in the TTY viewer.
- XML-compatible markup, including HTML-like documents, is formatted with
  structural indentation.

Other types are intentionally passthrough:

- Plain text is indexed and viewed as-is.
- Jinja templates are indexed and highlighted as templates, but `fmtview` does
  not render them, evaluate includes, or rewrite template statements.

Format a literal string:

```sh
fmtview --literal '{"a":{"b":1}}'
```

Write output:

```sh
fmtview data.xml > pretty.xml
fmtview page.html > pretty.html
fmtview template.html.j2 > template.copy.html.j2
fmtview app.log > app.log.copy
cat events.jsonl | fmtview --type jsonl > pretty.jsonl
```

JSONL input is still processed one record at a time, but each record is
pretty-printed with structural indentation. Deeply nested records expand across
multiple output lines.
Plain text and Jinja inputs are passthrough types: redirected stdout preserves
the input content instead of formatting it, while the TTY viewer still provides
scrolling, wrapping, search, and syntax highlighting where available.

Diff after applying each input's profile:

```sh
fmtview diff left.json right.json
fmtview diff left.xml right.xml > formatted.diff
fmtview diff --type jsonl old.jsonl new.jsonl
fmtview diff --type plain old.log new.log
```

In a terminal, `fmtview diff` opens an interactive diff viewer. Press `s` to
switch between single-column and side-by-side layouts, and `]`/`[` to jump to
the next or previous changed block. Long diff rows soft-wrap by default; press
`w` to switch to nowrap mode when exact columns matter. When stdout is
redirected, diff output remains standard unified patch text. The interactive
viewer hides patch control rows such as `@@` hunks and uses red/green
background shading for changed chunks, with stronger shading on the changed
portion inside each line. For record-stream inputs such as JSONL, the
interactive viewer opens lazily and continues scanning records in the
background instead of formatting both full inputs before the first draw.

## Try The Showcase Files

The repository includes small sample files that exercise the viewer features:

```sh
fmtview examples/showcase.json
fmtview examples/events.jsonl
fmtview examples/response.xml
fmtview examples/page.html
fmtview examples/template.html.j2
fmtview diff examples/diff-left.json examples/diff-right.json
```

Use the mouse wheel or trackpad to scroll, `Space`/`f` and `b` to page, `w` to
toggle wrap/nowrap, and `q` to exit. `examples/showcase.json` includes embedded
XML, a deliberately mismatched XML closing tag, escaped special tokens, nested
JSON, arrays, booleans, nulls, and an oversized single logical line near the top
of the file for wrapped scroll testing.
`examples/events.jsonl` includes a single deeply nested JSONL record on one
physical input line so you can verify that JSONL records are expanded by JSON
structure during formatting. `examples/page.html` is well-formed HTML that
exercises the XML-compatible markup formatter.
`examples/template.html.j2` exercises Jinja variables, blocks, comments, and
raw sections without rendering or reformatting template statements.
`examples/diff-left.json` and `examples/diff-right.json` are a paired diff
showcase with separated change blocks for trying the single-column/split layout
toggle, next/previous change navigation, and line/inline diff shading.

## Viewer

The viewer is intentionally small and works with both keyboard and pointer
input:

```text
q           quit
Esc         cancel a prompt/status message; otherwise quit
Wheel       scroll down/up by logical line
Trackpad    vertical scroll; horizontal scroll in nowrap mode
Shift+Wheel horizontal scroll in nowrap mode
/           search visible text
n/N         next/previous search match
Digits+Enter jump to a line number, for example 1200 Enter
Backspace   edit a pending prompt
j/k         scroll down/up by logical line
Up/Down     scroll down/up by logical line
Space/f     page down
b           page up
g/G         top/end when no prompt is pending
w           toggle wrap/nowrap
h/l         horizontal scroll in nowrap mode
Left/Right  horizontal scroll in nowrap mode
```

Diff viewer keys:

```text
s           toggle single-column/side-by-side diff layout
w           toggle wrap/nowrap
]/[         next/previous changed block
h/l         horizontal scroll in nowrap mode
Left/Right  horizontal scroll in nowrap mode
```

The title bar shows the source label, total line count, visible line range,
scroll percentage, and whether wrapping is enabled. In wrap mode, the percentage
tracks the visible byte position so it can advance inside a very long logical
line without scanning the whole file. When the viewport starts inside one
wrapped logical line, the title/footer also show a `+N rows` offset so repetitive
content still gives visible scrolling feedback. The left gutter shows line
numbers, and wrapped continuation rows use a lighter continuation gutter with
periodic tick marks.

For JSON-like transformed output, the viewer keeps a small key breadcrumb pinned
above the scrollable body, such as `payload › items › name`. Long paths wrap to
at most two compact rows and use cached checkpoints so normal scrolling does not
rescan the document from the beginning.

For record-like inputs such as JSONL logs, the terminal viewer formats records
on demand instead of formatting and indexing the whole file before the first
screen. While the lazy index is still growing, the title may show a `+` after
the line count; the viewer continues extending that session index during idle
time. Redirected output still performs the full deterministic formatting pass.

To jump to a specific line, type the line number directly and press Enter. While
a line jump is pending, the footer shows the target line; Backspace edits it and
Esc cancels it. Out-of-range line numbers are clamped to the file. On fully
indexed files, jumps seek through the formatted line-offset index instead of
scanning from the top, so jumping to a deep line only reads the target window.
On lazy record previews, jumps are bounded by the currently discovered session
index while background preloading continues to extend it.

To search, press `/`, type a substring, and press Enter. Search is
case-sensitive and runs over the visible text you are viewing. `fmtview` jumps
to the next matching line, then `n` and `N` repeat the search forward and
backward with wrap-around. Matches visible in the current viewport are
highlighted with a warm background without replacing JSON or markup syntax
colors.

Mouse capture is enabled while the viewer is open so wheel and trackpad events
go to `fmtview`. If your terminal uses mouse capture for selection, hold the
terminal's normal bypass modifier, usually Shift.

Soft wrap is enabled by default. Continuation rows preserve the original
indentation, with a capped extra indent so deeply nested documents still have
usable content width. Press `w` to switch to nowrap mode when exact columns
matter; horizontal scrolling is available there.

Syntax highlighting and wrapping are applied only to the visible window. That
means a very large file does not require a full highlighted render before you
can start scrolling.

## Markup

JSON often carries XML, XHTML, or other tag-shaped markup as string data:

```json
{
  "payload": "<root><item id=\"1\">value</item></root>"
}
```

`fmtview` keeps that string unchanged in transformed output, but the viewer still
tokenizes the markup inside it. Opening and closing tags are paired by depth, so
`<root>` and `</root>` share one color while nested tags use another. A local
mismatch such as `"<root></item>"` is highlighted as an error.

Standalone markup uses XML parsing rules. Well-formed XML, XHTML, and
XML-compatible HTML snippets are good inputs; browser-tolerant HTML that relies
on omitted closing tags, such as `<br>` or `<img>` without a closing slash or
end tag, should be normalized first.

## Performance Model

The product goal is a responsive CLI viewer, not a batch formatter with a pager
attached. `fmtview` keeps viewing, highlighting, searching, and diffing fast by
doing work at the same granularity the terminal can display. It does not keep
rendered output in memory for browsing.

- Normal formatted inputs are transformed into a temporary file.
- A compact byte-offset index is built for transformed or passthrough lines.
  The viewer uses that index to seek directly to the current window, which
  keeps paging and line jumps from rereading earlier content.
- Record-like TTY previews, such as JSONL logs, use a lazy path: `fmtview`
  sniffs a small prefix to confirm that the input is independent records, then
  formats only the records needed for the visible window.
- Lazy preview writes transformed records into a temporary spool and keeps compact
  offsets, not formatted strings, in memory. The title shows `N+` lines while
  the session index is still incomplete and idle time extends the index.
- Passthrough inputs, such as plain text and Jinja templates, are indexed
  without content rewriting.
- The terminal viewer uses compact ANSI redraws and avoids repainting invisible
  background cells during normal scrolling.
- Very long wrapped lines use terminal scroll regions for visual-row scrolling,
  so moving within one logical record only draws the newly visible rows.
- The viewer redraws on input or resize events, not on a fixed idle timer.
- Bursty keyboard, mouse wheel, and trackpad events are coalesced before redraw,
  so fast scrolling does not render one frame per raw terminal event.
- Scrolling reads and caches nearby lines around the current terminal window.
- Rendered visual rows are cached with a bounded, context-aware cache and
  prewarmed around the current viewport.
- Highlighting and wrapping scan only the visible prefix of long lines.
- Viewer search scans the indexed visible text in bounded chunks.
- JSON, JSONL, XML-compatible markup, plain text, and Jinja templates are
  processed incrementally where their load strategy allows it.
- JSON numbers are written from their original tokens instead of being coerced
  through native integer or floating-point types.

This keeps the viewer usable for large files while preserving scriptable stdout
behavior when you redirect output. Redirected formatting and diff output still
use the full deterministic formatting path rather than the lazy viewer path.

Maintainers can measure viewer rendering and terminal draw changes with:

```sh
benches/viewer-performance.sh
```

Interactive diff model and rendering changes can be measured with:

```sh
benches/diff-performance.sh
```

Parser, formatter, JSONL record, and lazy-preview changes can be measured with:

```sh
benches/format-performance.sh
```

Alternate complete-output formatter algorithms can be compared with:

```sh
benches/format-algorithm.sh --candidate 'experiment=target/release/fmtview --type {type} --indent {indent} {input}'
```

See `docs/performance.md` for the benchmark metrics and comparison workflow.

## CLI

```text
fmtview [OPTIONS] [INPUT]
fmtview diff [OPTIONS] <LEFT> <RIGHT>
```

Options:

```text
-t, --type <auto|json|jsonl|xml|plain|jinja>
                                  Override type-profile detection
    --literal <STRING>            Read this string instead of a file/stdin
    --indent <N>                  Pretty-print indent width, default 2
```

Use `-` or omit the input path to read stdin.
