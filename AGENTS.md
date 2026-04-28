# AGENTS.md

Principles for agents contributing to this repository.

## Core Principles

1. **KISS**
   - Prefer the simplest solution that satisfies the current CLI behavior.
   - Avoid speculative plugin systems, background daemons, or broad abstractions.
   - Keep modules small enough to read in one pass.

2. **Large-file behavior is a product requirement**
   - Do not load formatted output into memory for terminal viewing.
   - Prefer streaming parsers and temporary files with line indexes.
   - If a feature requires whole-document parsing, make that tradeoff explicit in the CLI or docs.

3. **Conventional Commits with real bodies**
   - Use Conventional Commits for every commit.
   - Include a body that explains what changed and why.

4. **Release notes are part of the release**
   - Maintain `CHANGELOG.md` for user-facing changes.
   - Every release version must have a `CHANGELOG.md` entry before tagging.
   - Do not publish a release with placeholder notes.

## Navigation

Use README for user-facing behavior. Use docs for maintainer workflows and
durable project decisions.

Keep this file coarse-grained. Do not mirror every implementation detail here.
Use `docs/INDEX.md` as the navigation entry point when you need code layout or
workflow-specific docs.

### Read these docs first

- `README.md`
- `docs/INDEX.md`
- `CHANGELOG.md`

### Read these docs when the task matches

- Release, packaging, crates.io, npm, GitHub Releases, or version tags:
  - Read `docs/releasing.md`

## Engineering Rules

- Keep stdout output valid and scriptable.
- Keep interactive behavior behind TTY detection.
- Preserve JSON string values. The viewer may highlight embedded XML, but formatted output must not rewrite string contents.
- Update `README.md` when CLI flags, install steps, or user-visible behavior changes.
- Update `CHANGELOG.md` when user-facing behavior, packaging, or release process changes.
- Update `docs/releasing.md` when release, packaging, npm, crates.io, or artifact policy changes.
- Prefer Linux-first behavior, but avoid unnecessary non-portable code when portable Rust is simple.

## Verification Requirements

- Run `cargo fmt`.
- Run `cargo test`.
- Run `cargo clippy --all-targets -- -D warnings` when Clippy is available.
- For TUI changes, run the built CLI under a real PTY, for example with `script`, and verify scroll/quit keys.
- For viewer rendering, wrapping, highlighting, or terminal draw performance changes, run `benches/viewer-performance.sh` and compare median time/byte counts.
- For parser, formatter, JSONL record, or lazy-preview performance changes, run `benches/format-performance.sh` and compare median timings.
- For alternate complete-output parser or formatter algorithms, run `benches/format-algorithm.sh --candidate 'name=...'` and require byte-for-byte aligned output.
