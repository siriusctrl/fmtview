# fmtview

Fast CLI viewing, highlighting, search, and diffing for JSON, JSONL,
XML-compatible markup, plain text, and Jinja templates.

This npm package installs the prebuilt static Linux x64 `fmtview` binary.

```sh
npm install -g fmtview
fmtview data.json
fmtview template.html.j2
fmtview app.log
```

`fmtview` resolves inputs through type profiles. Extensions such as `.json`,
`.jsonl`, `.html`, `.txt`, and `.j2` are hints, and unknown extensions are
sniffed from a bounded content prefix where possible.

JSON, JSONL/NDJSON, and XML-compatible markup are formatted. Plain text and
Jinja templates are passthrough types: they are indexed and previewed without
rewriting their content.

The main product surface is the terminal viewer: fast lazy loading, useful
highlighting, in-viewer search, and interactive diffs while redirected stdout
stays scriptable.

For source, full docs, and other installation options, see:

https://github.com/siriusctrl/fmtview
