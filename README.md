# fmtview

Fast terminal preview, formatting, and diffing for JSON, JSONL, and XML.

`fmtview` is built for the workflow where you want to inspect structured data in
a terminal first, and only write output when you explicitly redirect it.

```sh
fmtview payload.json
fmtview events.jsonl
fmtview response.xml
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
XML payloads or nested markup.

`fmtview` combines the two:

- Format JSON, JSONL, and XML from files, stdin, or literal strings.
- Preview in a terminal UI with line numbers, progress, and indent-aware soft
  wrap.
- Scroll with the keyboard, mouse wheel, or a trackpad without re-rendering on
  every individual input event.
- Highlight JSON, XML, and unified diff output.
- Highlight JSON string escape tokens such as `\n`, `\t`, `\r`, `\"`, and
  `\\`.
- Pair XML opening and closing tags by depth, including XML embedded inside JSON
  string values.
- Preserve data semantics. JSON strings are highlighted for readability, not
  rewritten.
- Keep large outputs responsive by indexing a temporary formatted file and only
  reading the visible window.

## Install

```sh
cargo install --git https://github.com/siriusctrl/fmtview
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
cat events.jsonl | fmtview --type jsonl > pretty.jsonl
```

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
fmtview diff examples/diff-left.json examples/diff-right.json
```

Use the mouse wheel or trackpad to scroll, `Space`/`f` and `b` to page, `w` to
toggle wrap/nowrap, and `q` to exit. `examples/showcase.json` includes embedded
XML, a deliberately mismatched XML closing tag, escaped special tokens, nested
JSON, arrays, booleans, nulls, and long strings for wrap testing.

## Viewer

The viewer is intentionally small and works with both keyboard and pointer
input:

```text
q/Esc       quit
Wheel       scroll down/up by logical line
Trackpad    vertical scroll; horizontal scroll in nowrap mode
Shift+Wheel horizontal scroll in nowrap mode
j/k         scroll down/up by logical line
Up/Down     scroll down/up by logical line
Space/f     page down
b           page up
Ctrl-d      half page down
Ctrl-u      half page up
g/G         top/end
w           toggle wrap/nowrap
h/l         horizontal scroll in nowrap mode
Left/Right  horizontal scroll in nowrap mode
```

The title bar shows the source label, total line count, visible line range,
scroll percentage, and whether wrapping is enabled. The left gutter shows line
numbers, and wrapped continuation rows use a lighter continuation gutter.

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

## Embedded XML

JSON often carries XML as string data:

```json
{
  "payload": "<root><item id=\"1\">value</item></root>"
}
```

`fmtview` keeps that string unchanged in formatted output, but the viewer still
tokenizes the XML inside it. Opening and closing tags are paired by depth, so
`<root>` and `</root>` share one color while nested tags use another. A local
mismatch such as `"<root></item>"` is highlighted as an error.

## Performance Model

`fmtview` does not keep the rendered output in memory for browsing.

- Input is formatted into a temporary file.
- A compact line-offset index is built once.
- The viewer redraws on input or resize events, not on a fixed idle timer.
- Bursty keyboard, mouse wheel, and trackpad events are coalesced before redraw,
  so fast scrolling does not render one frame per raw terminal event.
- Scrolling reads and caches nearby lines around the current terminal window.
- Rendered visual rows are cached with a bounded, context-aware cache and
  prewarmed around the current viewport.
- Highlighting and wrapping scan only the visible prefix of long lines.
- JSONL and XML are processed incrementally.
- JSON uses streaming JSON-to-JSON transcoding.

This keeps the viewer usable for large files while preserving scriptable stdout
behavior when you redirect output.

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
