# Changelog

All notable user-facing changes are documented here.

This project follows semantic versioning. Release entries are used as the source
for GitHub Release notes, so every published version must have a matching
`## [X.Y.Z]` section before the release tag is pushed.

## [Unreleased]

## [0.1.8] - 2026-04-28

### Fixed

- Place viewer search results with nearby context, including wrapped visual
  rows, instead of pinning the matching logical line or wrapped row to the top.

## [0.1.7] - 2026-04-28

### Changed

- Speed up terminal viewer rendering during scroll with a viewer-specific draw
  path, reusable buffers, and compact ANSI output.

### Fixed

- Keep muted viewer syntax colors without painting a full-screen background on
  every styled cell, avoiding bright terminal-default plain text while reducing
  repaint work.
- Improve auto lazy preview detection for `.jsonl`/`.ndjson` files while
  keeping ambiguous huge multi-line JSON documents on the full formatter path.

## [0.1.6] - 2026-04-27

### Changed

- Open record-like TTY previews lazily, formatting only the visible records
  first so very large JSONL/log-style files can show the first screen before the
  whole file is formatted and indexed, then continue extending the session
  index during idle time without retaining formatted lines in memory.
- Keep redirected formatting and diff output on the existing full deterministic
  path while the interactive viewer uses lazy preview data.
- Use an explicit muted RGB palette and One Dark-style background for the
  terminal viewer's syntax colors, diff markers, gutters, and search highlights
  so high-contrast terminal themes do not turn highlights into overly bright
  ANSI colors.
- Use a muted yellow search-match background so matches remain visible without
  returning to the previous bright highlight color.

## [0.1.5] - 2026-04-27

### Changed

- Improved wrapped long-line viewer performance by caching rendered visual row
  chunks and prewarming nearby chunks without reducing syntax highlight
  correctness.
- Added lightweight wrap and XML-like highlight checkpoints for huge logical
  lines, so repeated deep scrolling rebuilds from nearby state instead of the
  start of the line.
- Optimized tail-page calculation and rendering for jumps near huge wrapped
  lines while keeping the last full page visible.
- Clamped wrapped EOF row offsets to the actual final full page instead of
  preserving overscrolled partial pages.
- Fixed `End`/`G` in wrap mode so single-line long records jump to the real
  final visual page and report 100% at EOF.
- Added wrapped-line position feedback with `+N rows` status text and subtle
  continuation gutter ticks for very long repeated records.
- Reduced large-record indexing overhead by scanning formatted output in fixed
  byte buffers instead of allocating per physical line during index creation.
- Added an ignored release-mode performance smoke test for huge wrapped-line
  rendering and last-line jump paths.

## [0.1.4] - 2026-04-27

### Changed

- Documented the changelog as the source of release notes.
- Updated the release workflow to publish GitHub Release notes from this file.
- Improved wrapped-line scrolling so an overlong logical line can be inspected
  before moving to the next line.
- Fixed mouse-wheel scrolling over wrapped lines so repeated wheel events do
  not overshoot into an invalid row offset.
- Fixed the viewer title range and progress percentage to use the logical lines
  actually rendered after wrapping.
- Updated wrap-mode progress to use visible byte position, so it advances
  inside a long wrapped logical line without scanning the whole file.
- Updated the wrap toggle footer hint to show `w unwrap` while wrapping is on
  and `w wrap` while wrapping is off.
- Added an oversized wrapped-line case near the top of `examples/showcase.json`.

## [0.1.3] - 2026-04-27

### Changed

- Pretty-print each JSONL record using JSON structure instead of preserving each
  record as one physical output line.
- Generalized XML wording to XML-compatible markup and documented the
  well-formed markup boundary for HTML-like inputs.
- Added `.html`, `.htm`, and `.xhtml` auto-detection through the existing
  XML-compatible markup formatter.

### Added

- Added a deeply nested JSONL record to `examples/events.jsonl` as a regression
  showcase.
- Added `examples/page.html` for well-formed HTML markup formatting.

## [0.1.2] - 2026-04-27

### Changed

- Switched Linux x64 release artifacts and npm binaries to the
  `x86_64-unknown-linux-musl` target so npm installs do not depend on the host
  system glibc version.
- Upgraded release artifact actions to Node 24-compatible versions.

## [0.1.1] - 2026-04-24

### Fixed

- Preserved JSON numeric tokens without coercing through native integer or
  floating-point types.
- Kept redirected equal diffs machine-readable by emitting empty stdout for no
  differences.
- Restored terminal state on viewer setup failures.

### Changed

- Removed the non-unified streaming diff fallback.

## [0.1.0] - 2026-04-23

### Added

- Initial public release of `fmtview`.
- Formatting and terminal viewing for JSON, JSONL, and XML.
- Unified diff output after formatting both inputs.
- Syntax highlighting for JSON, XML, and unified diffs.
- XML tag pairing inside standalone XML and JSON string values.
- Indent-aware soft wrapping, mouse and trackpad scrolling, line jumping, and
  in-viewer search.
- GitHub Release, crates.io, and npm Linux x64 distribution.
