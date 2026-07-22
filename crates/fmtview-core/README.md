# fmtview-core

`fmtview-core` is the headless viewer engine used by the `fmtview` CLI.

The crate owns format profiles, transforms, indexed and lazy loading, viewer
and diff state, navigation, search, highlighting, layout, render caches, and
backend-neutral render frames. Terminal setup, event adaptation, frame commit,
CLI parsing, and shell integration remain in the `fmtview` package.

The crates are versioned together. Most users should install the `fmtview`
binary rather than depend on this crate directly.
