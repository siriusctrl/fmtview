# fmtview-core

`fmtview-core` is the headless viewer engine used by the `fmtview` CLI.

The crate owns format profiles, transforms, indexed and lazy loading, viewer
and diff state, navigation, search, highlighting, layout, render caches, and
backend-neutral render frames. Terminal setup, event adaptation, frame commit,
CLI parsing, and shell integration remain in the `fmtview` package.

It also exposes `RecordTimeline`, a backend-neutral bidirectional source seam
for committed record streams. Implementations provide a stable label and
snapshot, bounded older/newer reads, exact raw record bytes with stable
epoch/offset identities, explicit post-batch `More`/`Pending`/`End` boundaries,
and append/pending/end/reset refresh outcomes.
`RecordTimelineViewFile` adds formatting and indexed on-disk spooling;
`FileRecordTimeline` supplies a tail-first growing JSONL/NDJSON implementation.
The core viewer owns viewport-anchor preservation and follow state through
backend-neutral events, including `ViewerCommand::FollowTail` and
`ViewerCommand::ToggleFollowTail`.

Record-backed views can expose exact raw-record snapshots through bounded
virtual lines. Lazy JSONL records are copied once at ingest into an immutable
raw spool, so opening raw mode is an offset lookup and source replacement
cannot change the snapshot. `FileViewer` owns the `r` structured/raw toggle,
separate raw search and wrap state, and synchronization when returning to the
structured viewport. Follow refresh and the main viewport's attached or
detached state continue behind an open raw snapshot.

Generic conversation handling also stays in core. The JSON package recognizes
direct chat roles, contextual tool calls, direct tool-role results, and nested
typed tool results without depending on an application's storage types. The
viewer-only JSONL display path can collapse explicitly typed inline base64
media to media type and validated decoded size without decoding the payload.
Normal transforms preserve exact tool-argument tokens, media payloads, and
unknown fields for redirected output and diffs.

Reset overlap uses a bounded, non-consuming probe of the committed source
prefix. Exact IDs for the matched prefix are filtered from tail and later older
batches, so large replacements remain tail-first without guessing from equal
records in the middle of a stream.

The trait contains no file-specific checkpoint policy or terminal backend
method, so embedders can supply a different committed-record source without
adopting crossterm, poll timing, or the CLI's filesystem behavior.

The crates are versioned together. Most users should install the `fmtview`
binary rather than depend on this crate directly.
