# fmtview

Fast CLI viewing, highlighting, search, and diffing for JSON, JSONL,
XML-compatible markup, HTML, Markdown, TOML, plain text, and Jinja templates.

`fmtview` is built for the workflow where you want to inspect data quickly in a
terminal: open large files without waiting for a full render, keep format
highlighting useful, search the visible text, and diff formatted or passthrough
inputs without leaving the CLI.

```sh
fmtview payload.json
fmtview events.jsonl
fmtview --follow events.jsonl
fmtview response.xml
fmtview page.html
fmtview notes.md
fmtview config.toml
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
- Highlight JSON, XML-compatible markup, HTML, embedded markup in JSON strings,
  Markdown, TOML, and Jinja templates.
- Search the visible text without loading rendered output into memory.
- Diff inputs after applying each input's profile, with interactive
  single-column and side-by-side layouts in a TTY and unified patches on
  redirected stdout.
- Format JSON, JSONL, XML-compatible markup, and HTML when that is the right content
  strategy.
- Preview Markdown, TOML, plain text, and Jinja templates without rewriting
  their content.
- Scroll with the keyboard, mouse wheel, or a trackpad without re-rendering on
  every individual input event.
- Highlight JSON string escape tokens such as `\n`, `\t`, `\r`, `\"`, and
  `\\`.
- Pair XML-style opening and closing tags by depth, including markup embedded
  inside JSON string values.
- Keep a compact sticky JSON key breadcrumb above the viewer body while
  scrolling nested documents.
- Show a compact chat role gutter for JSON and JSONL message objects with
  `system`, `user`, `assistant`, or `tool` roles. The `S`/`U`/`A`/`T` label
  marks the object start, and its colored guide covers the body between the
  opening and closing braces. Boundary rows and objects without a direct role
  stay neutral. Role-bearing objects are also preferred for structure jumps.
- Match direct `role: tool` results and nested typed `tool_result` objects to
  recent earlier tool calls by exact ID in common `tool_call_id`/`tool_use_id`,
  `call_id`, and contextual custom fields. Matched
  calls and results reuse the existing line-number separator for compact
  `↓`/`↑` direction markers, without taking another column; press `t` to jump
  exactly between a matched pair.
- Collapse high-confidence inline base64 media in the interactive JSONL view to
  a media type and validated decoded byte size, without allocating a decoded
  payload. Press `r` to inspect the current record's original source text and
  press it again to return to the structured view.
- Preserve data semantics. JSON strings are highlighted for readability, not
  rewritten.
- Keep large outputs responsive by indexing a temporary text file and only
  reading the visible window.

The design is viewer-first rather than extension-first. Each input resolves to
a profile that chooses the shared runtime strategy and the format package needed
for the current use case:

```text
  Use case + input type
          |
          v
  +------------------+
  | TypeProfile      |
  | - content kind   |
  | - input shape    |
  | - load strategy  |
  | - transform plan |
  | - format package |
  +---------+--------+
            |
            v
  +---------+----------+--------------+-----------+
  | indexed loading    | transformed stdout       |
  | visible highlight  | searchable viewer + diff |
  +--------------------+--------------------------+
```

That is why JSON and XML can be formatted, JSONL can open lazily record by
record, and Markdown, TOML, plain text, or Jinja templates can stay passthrough
while still getting fast view/search/diff behavior. See `docs/architecture.md`
for the maintainer-facing design notes.

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

### Short Alias

`fmtview` can print or install a shell alias when you want a shorter daily
command:

```sh
fmtview alias bash
fmtview alias zsh
fmtview alias fish
```

By default this only prints the snippet, for example `alias fv='fmtview'`.
Use `-i`/`--install` to write a managed block into the shell startup file:

```sh
fmtview alias zsh -i
fmtview alias fish --install
```

If `fv` already exists on `PATH`, installation stops instead of overwriting it.
Choose another name when needed:

```sh
fmtview alias zsh -i --name fmtv
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

`fmtview` resolves every input to a type profile: content kind, input shape,
load strategy, transform strategy, and format package. File extensions are only
one signal. When the extension is unknown, `fmtview` sniffs a bounded
prefix of the content: JSON-looking documents use the JSON formatter, record
streams use the lazy JSONL path, markup-looking documents use the XML-compatible
or HTML formatter, and otherwise the input falls back to plain-text passthrough.

If an auto-detected structured type cannot be formatted in the interactive
viewer, `fmtview` falls back to plain-text viewing and shows a temporary red
footer notice instead of closing with a parse error. Use `--type` when you want
to force a specific profile. Redirected stdout remains strict and reports
formatting errors instead of silently changing output semantics.

Known extensions still provide a fast, deterministic hint:

- `.json` -> JSON formatting.
- `.jsonl` and `.ndjson` -> lazy JSONL record formatting.
- `.xml` and `.xhtml` -> XML-compatible markup formatting.
- `.html` and `.htm` -> HTML formatting with a tolerant HTML5-style parser.
- `.md`, `.markdown`, `.mdown`, and `.mkd` -> Markdown passthrough with
  Markdown highlighting in the TTY viewer. Fenced `json`, `jsonl`, `toml`,
  `xml`/`html`, and `jinja` blocks reuse the matching viewer highlighter.
- `.toml` -> TOML passthrough with TOML highlighting in the TTY viewer.
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
  In the interactive viewer, malformed records are shown as raw text with a
  temporary red notice while later records continue formatting normally.
- XML-compatible markup is formatted with structural indentation.
- HTML is formatted with structural indentation using a tolerant tokenizer:
  void elements (`<br>`, `<img>`, ...), optional closing tags (`<p>`, `<li>`,
  `<td>`, ...), unquoted attributes, and raw-text elements (`<script>`,
  `<style>`, `<pre>`, `<textarea>`, `<title>`) are accepted. Markup and
  text-node content are preserved, while formatting-only whitespace between
  elements may be normalized into one-element-per-line indentation. Missing
  close tags are not synthesized and stray close tags are not dropped.

Other types are intentionally passthrough:

- Markdown is indexed, wrapped, and highlighted, but not rendered to HTML or
  reformatted. Known fenced code blocks reuse the same highlighters as
  top-level files.
- TOML is indexed and highlighted, but not reformatted.
- Plain text is indexed and viewed as-is.
- Jinja templates are indexed and highlighted as templates, but `fmtview` does
  not render them, evaluate includes, or rewrite template statements.

### Conversation records and inline media

Conversation-shaped JSON remains ordinary JSON: direct `role`/`ref` fields,
typed `content` items, reasoning, runtime reminders, artifact metadata, and
unknown future fields are formatted and displayed instead of projected into a
fixed schema. Typed `tool_call`/`tool_use` and `tool_result`/`tool_response`
objects can link by exact IDs even when they are nested in `content` arrays.
Embedded argument strings keep their original JSON token spelling and escapes.

In the interactive JSONL viewer, a `data:<media-type>;base64,...` string is
shown as a compact media summary. A same-object `type: base64` plus
`media_type`/`mime_type` and later `data` field is handled too. Metadata must
precede that sibling `data` field for streaming recognition. Valid payloads
show the decoded byte size; recognized invalid payloads say `invalid base64`
instead of claiming a size. Redirected output never collapses these strings.

### Following growing JSONL files

Use `-F`/`--follow` with a JSONL or NDJSON file to open directly at its current
committed tail:

```sh
fmtview --follow service.jsonl
```

The first frame reverse-scans only enough source data to build the last
viewport; it does not format or index the file from the beginning. A final
record is committed only after its newline arrives, so a writer's partial EOF
record is never displayed early. While the viewport remains at the bottom,
committed appends advance it automatically. Scrolling up changes the footer to
`follow:detached`; scrolling back to the physical bottom reattaches. Press `f`
to pause or resume follow explicitly. Search plus structure, chat-role, and tool
navigation continue across older records loaded on demand and records appended
after opening.

Follow mode requires a real file and an interactive stdout terminal. Ordinary
viewing and redirected output are unchanged; `--follow` never turns redirected
stdout into an endless stream.

Format a literal string:

```sh
fmtview --literal '{"a":{"b":1}}'
```

Write output:

```sh
fmtview data.xml > pretty.xml
fmtview page.html > pretty.html
fmtview notes.md > notes.copy.md
fmtview config.toml > config.copy.toml
fmtview template.html.j2 > template.copy.html.j2
fmtview app.log > app.log.copy
cat events.jsonl | fmtview --type jsonl > pretty.jsonl
```

JSONL input is still processed one record at a time, but each record is
pretty-printed with structural indentation. Deeply nested records expand across
multiple output lines.
Markdown, TOML, plain text, and Jinja inputs are passthrough types: redirected
stdout preserves the input content instead of formatting it, while the TTY
viewer still provides scrolling, wrapping, search, and format highlighting where
available.

Diff after applying each input's profile:

```sh
fmtview diff left.json right.json
fmtview diff left.xml right.xml > formatted.diff
fmtview diff --type jsonl old.jsonl new.jsonl
fmtview diff --type markdown old.md new.md
fmtview diff --type toml old.toml new.toml
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
fmtview examples/chat.jsonl
fmtview examples/conversation.jsonl
fmtview examples/long-inline.jsonl
fmtview examples/response.xml
fmtview examples/page.html
fmtview examples/messy.html
fmtview examples/notes.md
fmtview examples/config.toml
fmtview examples/template.html.j2
fmtview diff examples/diff-left.json examples/diff-right.json
```

Use the mouse wheel or trackpad to scroll, `Space`/`f` and `b` to page, `w` to
toggle wrap/nowrap, `m` to toggle terminal text selection, and `q` to exit.
`examples/showcase.json` includes embedded
XML, a deliberately mismatched XML closing tag, escaped special tokens, nested
JSON, arrays, booleans, nulls, and an oversized single logical line near the top
of the file for wrapped scroll testing.
`examples/events.jsonl` includes a single deeply nested JSONL record on one
physical input line so you can verify that JSONL records are expanded by JSON
structure during formatting. `examples/long-inline.jsonl` is generated by
`examples/generate-long-inline.py` and includes several JSONL records with very
long string values that fill multiple wrapped terminal viewports, which is
useful for testing structure jumps around partially observed inline content.
`examples/chat.jsonl` includes top-level and nested chat-style message objects,
mixed `system`/`user`/`assistant`/`tool` ordering, an object without a role, and
a matched tool call/result pair for testing role scopes, relation markers,
pair navigation, and chat-aware structure jumps.
`examples/conversation.jsonl` includes generic typed content, nested tool
call/result objects, an exact embedded arguments string, reasoning/runtime and
artifact metadata, and same-object base64 media.
`examples/page.html` is well-formed HTML that also works as XML-compatible
markup.
`examples/messy.html` is loose HTML with void elements, unclosed optional
tags, unquoted attributes, and raw `<script>`/`<pre>` content so you can try
the tolerant HTML tokenizer.
`examples/template.html.j2` exercises Jinja variables, blocks, comments, and
raw sections without rendering or reformatting template statements.
`examples/notes.md` exercises Markdown headings, lists, blockquotes, links,
inline code, and fenced code with nested highlighting, without rendering the
Markdown document.
`examples/config.toml` exercises TOML sections, keys, strings, arrays, numbers,
and booleans without reformatting the file.
`examples/diff-left.json` and `examples/diff-right.json` are a paired diff
showcase with separated change blocks for trying the single-column/split layout
toggle, next/previous change navigation, and line/inline diff shading.

## Viewer

The viewer is intentionally small and works with both keyboard and pointer
input:

```text
q           quit
Esc         cancel a prompt/status message; otherwise quit
Wheel       scroll one visual row in wrap mode; one logical line in nowrap
Trackpad    vertical scroll; horizontal scroll in nowrap mode
Shift+Wheel horizontal scroll in nowrap mode
/           search visible text
n/N         next/previous search match
]/[         next/previous structure
t           jump between the focused tool call/result pair
r           toggle current JSONL record between structured and raw source view
m           toggle mouse selection mode
Digits+Enter jump to a line number, for example 1200 Enter
Backspace   edit a pending prompt
j/k         scroll one visual row in wrap mode; one logical line in nowrap
Up/Down     scroll one visual row in wrap mode; one logical line in nowrap
Space       page down
f           toggle follow-tail with --follow; page down otherwise
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
time. If a JSONL record is malformed, the TTY viewer keeps that record as raw
text, shows a temporary red notice, and continues with following records.
Redirected output still performs the full deterministic formatting pass.
With `--follow`, this same indexed spool starts from the committed tail, loads
older records backward when needed, and extends forward after source refreshes.
The footer shows `follow:on`, `follow:detached`, or `follow:off`.

Press `r` on a JSONL/NDJSON record to open a bounded raw snapshot backed by its
exact source or spool range. An active search match selects its containing
record; otherwise the viewport's top record is used. The snapshot stays on that
record while a followed source appends or resets in the background; returning
to structured mode shows the updated stream without changing an existing
detached anchor. Raw display
uses 32 KiB visual chunks so a huge record is never copied wholesale into the
viewer frame. Search works within those chunks, but a query spanning an
artificial chunk boundary is not matched. Invalid UTF-8 is displayed lossily;
the exact bytes remain in the source/spool and are never reconstructed from
pretty output. Press `q` (or an idle `Esc`) to quit from either view.

To jump to a specific line, type the line number directly and press Enter. While
a line jump is pending, the footer shows the target line; Backspace edits it and
Esc cancels it. Out-of-range line numbers are clamped to the file. On fully
indexed files, jumps seek through the formatted line-offset index instead of
scanning from the top, so jumping to a deep line only reads the target window.
On lazy record previews, jumps are bounded by the currently discovered session
index while background preloading continues to extend it.

To jump between structures, press `]` or `[`. The viewer treats this as a
ranked structure jump rather than a simple line scan: stable landmarks such as
JSONL record starts, JSON array item objects, chat-style JSON message objects,
Markdown headings, TOML tables, Jinja blocks, and plain-text paragraph starts
can be selected even when they are already visible, so repeated jumps move
through the document's outline instead of only paging to unseen content. For
JSON and JSONL, an object with a direct `"role"` value of `"system"`, `"user"`,
`"assistant"`, or `"tool"` is treated as a chat message even when that object is
nested inside another field. In the normal JSON/JSONL viewer, these message
objects also show a compact `S`/`U`/`A`/`T` role gutter next to the line-number
gutter, so turn boundaries remain visible without relying only on token colors.
The role letter stays on the opening-brace row; the colored guide starts on the
next row and stops before the closing-brace row, leaving a neutral separation
between adjacent objects. The gutter is automatically hidden in narrow
terminals to preserve content width, and selection mode hides it along with
line numbers.

Tool calls and tool results reuse the existing line-number separator instead
of adding a separate relation lane. The matcher only considers ID-like fields
in a tool call object, a direct `"role": "tool"` result object, or a nested
typed `tool_result`/`tool_response` object, then requires
an exact ID value match with an earlier call from bounded recent context. On a
cold jump, context recovery reads at most 4,096 preceding lines. A matched call
replaces `│` with `↓`, and its result replaces it with `↑`; the
`S`/`U`/`A`/`T` label and role guide stay in the same columns. Unmatched and
ambiguous results keep the normal separator, while the footer still explains
their state. For matched pairs, the footer shows the ID and opposite line
number. Press `t` to jump to the other endpoint and press it again to return.
The direction markers add no horizontal width, including in narrow terminals
where the role gutter is hidden.

Visibility
still matters for detail blocks: small fully visible JSON object/array fields
are skipped, while larger composite fields, partially wrapped blocks, and
horizontally clipped blocks remain jump targets. XML/HTML jumps to start tags
when the tag spans more than the current line or is not fully observed.

To search, press `/`, type a substring, and press Enter. Search is
case-sensitive and runs over the visible text you are viewing. `fmtview` jumps
to the next matching line, then `n` and `N` repeat the search forward and
backward with wrap-around. Matches visible in the current viewport are
highlighted with a warm background without replacing JSON or markup colors
colors. The footer shows the current match ordinal and total from the session
search index, such as `2/8 matches`. A `+` suffix means the count is still
growing as lazy line indexing or background match counting advances. If a match
is beyond the counted prefix, the ordinal appears after the lazy index catches
up instead of blocking the viewer.

Mouse capture is enabled while the viewer is open so wheel and trackpad events
go to `fmtview`. Press `m` to release mouse capture when you want native
terminal text selection and copying. Selection mode hides the viewer frame and
line-number gutter so dragging over body rows copies the visible text rather
than the UI chrome. Press `m` again to restore viewer mouse and trackpad
handling.

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

Standalone markup resolves to either XML or HTML. `.xml` and `.xhtml` use the
strict XML parser. `.html` and `.htm` use the tolerant HTML tokenizer, which
accepts void elements, optional closing tags, and unquoted attributes while
preserving markup and text-node content. When the extension is unknown,
`fmtview` sniffs a bounded prefix to pick between the two (`<?xml` and `xmlns`
lean XML; `<!doctype html>`, an `<html>` root, or complete lowercase
unsaturated void elements such as `<br>` lean HTML).
XML-compatible HTML snippets are good inputs for the XML path; looser HTML
that relies on omitted closing tags, such as `<br>` or `<img>` without a
closing slash or end tag, should use the HTML profile (`fmtview --type html`)
or the `.html`/`.htm` extension.

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
- Follow-mode JSONL opens from a bounded reverse tail scan. Refresh locates the
  newest committed delimiter from EOF, and record/byte budgets bound subsequent
  older and newer loads; no persistent full-file index is required.
- Lazy preview writes transformed records into a temporary spool and keeps compact
  offsets, not formatted strings, in memory. The title shows `N+` lines while
  the session index is still incomplete and idle time extends the index.
- Viewer-only JSONL formatting scans recognized base64 payloads in buffered
  slices, validates padding/alphabet, and writes only the media summary. It does
  not allocate a decoded buffer or a payload-sized formatted output buffer.
- Passthrough inputs, such as Markdown, TOML, plain text, and Jinja templates,
  are indexed without content rewriting.
- The terminal viewer uses compact ANSI redraws and avoids repainting invisible
  background cells during normal scrolling.
- Very long wrapped lines use terminal scroll regions for visual-row scrolling,
  so moving within one logical record only draws the newly visible rows.
- The viewer redraws on input or resize events, not on a fixed idle timer.
- Bursty keyboard, mouse wheel, and trackpad events are coalesced before redraw,
  so fast scrolling does not render one frame per raw terminal event.
- Scrolling reads and caches nearby lines around the current terminal window.
- JSON/JSONL role and tool-link context is resolved in bounded, checkpointed
  windows and reused across adjacent scroll positions instead of rescanning
  the same lookahead on every row.
- Rendered visual rows are cached with a bounded, context-aware cache and
  prewarmed around the current viewport.
- Render-cache prewarming yields whenever more terminal input is already
  queued, so a burst of scroll events is not delayed by speculative work.
- Highlighting and wrapping scan only the visible prefix of long lines.
- Viewer search scans the indexed visible text in bounded chunks.
- JSON, JSONL, XML-compatible markup, HTML, Markdown, TOML, plain text, and Jinja
  templates are processed incrementally where their load strategy allows it.
- JSON numbers are written from their original tokens instead of being coerced
  through native integer or floating-point types.

This keeps the viewer usable for large files while preserving scriptable stdout
behavior when you redirect output. Redirected formatting and diff output still
use the full deterministic formatting path rather than the lazy viewer path.
The interactive viewer may fall back from failed auto-detected formatting to
plain-text viewing, but redirected stdout does not.

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
fmtview alias [OPTIONS] <bash|zsh|fish>
```

Options:

```text
-t, --type <auto|json|jsonl|xml|html|markdown|toml|plain|jinja>
                                  Override type-profile detection
-F, --follow                     Open a JSONL/NDJSON file at its tail and follow
    --literal <STRING>            Read this string instead of a file/stdin
    --indent <N>                  Pretty-print indent width, default 2
```

Alias options:

```text
-i, --install                     Install the alias into the shell startup file
    --name <NAME>                 Alias name, default fv
```

Use `-` or omit the input path to read stdin.
