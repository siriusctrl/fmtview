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

Reset overlap uses a bounded, non-consuming probe of the committed source
prefix. Exact IDs for the matched prefix are filtered from tail and later older
batches, so large replacements remain tail-first without guessing from equal
records in the middle of a stream.

The trait contains no file-specific checkpoint policy or terminal backend
method, so embedders can supply a different committed-record source without
adopting crossterm, poll timing, or the CLI's filesystem behavior.

The crates are versioned together. Most users should install the `fmtview`
binary rather than depend on this crate directly.
