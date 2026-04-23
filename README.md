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
- Preview in a terminal UI with line numbers, progress, and horizontal scroll.
- Highlight JSON, XML, and unified diff output.
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

## Viewer

The viewer is intentionally small and keyboard-driven:

```text
q/Esc       quit
j/k         scroll down/up
Up/Down     scroll down/up
PgUp/PgDn   page scroll
g/G         top/end
h/l         horizontal scroll
Left/Right  horizontal scroll
```

The title bar shows the source label, total line count, visible line range,
scroll percentage, and horizontal offset. The left gutter shows line numbers.

Syntax highlighting is applied only to the visible window. That means a very
large file does not require a full highlighted render before you can start
scrolling.

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
- Scrolling reads only the lines needed for the current terminal window.
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
