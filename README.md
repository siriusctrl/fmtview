# fmtview

`fmtview` is a Linux-first CLI for formatting, diffing, and syntax-highlighted
scroll-viewing JSON, JSONL, and XML without loading the rendered output into
memory.

## Install

From this repository:

```sh
cargo install --git https://github.com/siriusctrl/fmtview
```

For local development:

```sh
cargo build
cargo test
```

## Usage

Format a file. If stdout is a terminal, this opens the scrollable viewer; if
stdout is redirected, it prints formatted text.

```sh
fmtview data.json
fmtview data.jsonl
fmtview data.xml
```

Write formatted output with normal shell redirection:

```sh
fmtview data.json > pretty.json
cat data.xml | fmtview --type xml > pretty.xml
fmtview --literal '{"a":{"b":1}}' > pretty.json
```

Diff two inputs after formatting:

```sh
fmtview diff left.json right.json
fmtview diff left.xml right.xml > formatted.diff
fmtview diff --type jsonl old.jsonl new.jsonl
```

Viewer keys:

```text
q/Esc      quit
j/k        scroll down/up
Up/Down    scroll down/up
PgUp/PgDn  page scroll
g/G        top/end
h/l        horizontal scroll
Left/Right horizontal scroll
```

The viewer renders line numbers, scroll progress, and lightweight JSON/XML/diff
syntax highlighting for the visible window only. That keeps very large files
responsive while still making nested structures easier to scan.

## Embedded Strings

JSON strings are preserved by default, even if a value contains XML or JSON.
That keeps stdout output semantically safe.

For readability, opt in to recursively pretty-print string values that contain
JSON or XML:

```sh
fmtview --expand-embedded payload.json > readable.json
```

This mode is useful for inspection, but it can change string contents by adding
whitespace and newlines.

## Large Files

`fmtview` formats inputs into temporary files, builds a compact line-offset
index, and only reads the visible terminal window while scrolling. JSONL and XML
are processed incrementally. Regular JSON uses streaming JSON-to-JSON
transcoding unless `--expand-embedded` is enabled.
