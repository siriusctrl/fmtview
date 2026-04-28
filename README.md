# fmtview

Fast terminal preview, formatting, and diffing for JSON, JSONL, and
XML-compatible markup.

`fmtview` is built for the workflow where you want to inspect structured data in
a terminal first, and only write output when you explicitly redirect it.

```sh
fmtview payload.json
fmtview events.jsonl
fmtview response.xml
fmtview page.html
fmtview diff old.json new.json
```

If stdout is a terminal, `fmtview` opens an interactive viewer. If stdout is
redirected, it writes formatted text or diff output:

```sh
fmtview payload.json > pretty.json
fmtview diff old.json new.json > changes.diff
```

## Why

Pretty-printers are useful, but they usually dump text and hand scrolling to
your pager. Pagers are useful, but they do not understand JSON strings that hide
XML payloads, XHTML snippets, or nested markup.

`fmtview` combines the two:

- Format JSON, JSONL, and XML-compatible markup from files, stdin, or literal
  strings.
- Preview in a terminal UI with line numbers, progress, and indent-aware soft
  wrap.
- Scroll with the keyboard, mouse wheel, or a trackpad without re-rendering on
  every individual input event.
- Highlight JSON, XML-compatible markup, and unified diff output.
- Highlight JSON string escape tokens such as `\n`, `\t`, `\r`, `\"`, and
  `\\`.
- Pair XML-style opening and closing tags by depth, including markup embedded
  inside JSON string values.
- Search formatted text from inside the viewer with visible match highlighting.
- Preserve data semantics. JSON strings are highlighted for readability, not
  rewritten.
- Keep large outputs responsive by indexing a temporary formatted file and only
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

Format a literal string:

```sh
fmtview --literal '{"a":{"b":1}}'
```

Write formatted output:

```sh
fmtview data.xml > pretty.xml
fmtview page.html > pretty.html
cat events.jsonl | fmtview --type jsonl > pretty.jsonl
```

JSONL input is still processed one record at a time, but each record is
pretty-printed with structural indentation. Deeply nested records expand across
multiple output lines.

Diff after formatting both sides:

```sh
fmtview diff left.json right.json
fmtview diff left.xml right.xml > formatted.diff
fmtview diff --type jsonl old.jsonl new.jsonl
```

## Try The Showcase Files

The repository includes small sample files that exercise the viewer features:

```sh
fmtview examples/showcase.json
fmtview examples/events.jsonl
fmtview examples/response.xml
fmtview examples/page.html
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

## Viewer

The viewer is intentionally small and works with both keyboard and pointer
input:

```text
q           quit
Esc         cancel a prompt/status message; otherwise quit
Wheel       scroll down/up by logical line
Trackpad    vertical scroll; horizontal scroll in nowrap mode
Shift+Wheel horizontal scroll in nowrap mode
/           search formatted text
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

The title bar shows the source label, total line count, visible line range,
scroll percentage, and whether wrapping is enabled. In wrap mode, the percentage
tracks the visible byte position so it can advance inside a very long logical
line without scanning the whole file. When the viewport starts inside one
wrapped logical line, the title/footer also show a `+N rows` offset so repetitive
content still gives visible scrolling feedback. The left gutter shows line
numbers, and wrapped continuation rows use a lighter continuation gutter with
periodic tick marks.

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
case-sensitive and runs over the formatted text you are viewing. `fmtview` jumps
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

`fmtview` keeps that string unchanged in formatted output, but the viewer still
tokenizes the markup inside it. Opening and closing tags are paired by depth, so
`<root>` and `</root>` share one color while nested tags use another. A local
mismatch such as `"<root></item>"` is highlighted as an error.

Standalone markup uses XML parsing rules. Well-formed XML, XHTML, and
XML-compatible HTML snippets are good inputs; browser-tolerant HTML that relies
on omitted closing tags, such as `<br>` or `<img>` without a closing slash or
end tag, should be normalized first.

## Performance Model

`fmtview` does not keep the rendered output in memory for browsing.

- Normal inputs are formatted into a temporary file.
- A compact byte-offset index is built for formatted lines. The viewer uses that
  index to seek directly to the current window, which keeps paging and line
  jumps from rereading earlier content.
- Record-like TTY previews, such as JSONL logs, use a lazy path: `fmtview`
  sniffs a small prefix to confirm that the input is independent records, then
  formats only the records needed for the visible window.
- Lazy preview writes formatted records into a temporary spool and keeps compact
  offsets, not formatted strings, in memory. The title shows `N+` lines while
  the session index is still incomplete and idle time extends the index.
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
- Viewer search scans the indexed formatted file in bounded chunks.
- JSON, JSONL, and XML-compatible markup are processed incrementally.
- JSON numbers are written from their original tokens instead of being coerced
  through native integer or floating-point types.

This keeps the viewer usable for large files while preserving scriptable stdout
behavior when you redirect output. Redirected formatting and diff output still
use the full deterministic formatting path rather than the lazy viewer path.

Maintainers can measure viewer rendering and terminal draw changes with:

```sh
scripts/bench-viewer-performance.sh
```

See `docs/performance.md` for the benchmark metrics and comparison workflow.

## CLI

```text
fmtview [OPTIONS] [INPUT]
fmtview diff [OPTIONS] <LEFT> <RIGHT>
```

Options:

```text
-t, --type <auto|json|jsonl|xml>  Override format detection
    --literal <STRING>            Read this string instead of a file/stdin
    --indent <N>                  Pretty-print indent width, default 2
```

Use `-` or omit the input path to read stdin.
