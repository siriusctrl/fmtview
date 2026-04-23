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

## Source Map

- `src/main.rs` - CLI argument parsing and command routing.
- `src/input.rs` - file, stdin, and literal input materialization.
- `src/format.rs` - JSON, JSONL, XML formatting and auto-detection.
- `src/diff.rs` - formatted unified diff generation.
- `src/line_index.rs` - temp-file line offset indexing for large outputs.
- `src/viewer.rs` - interactive terminal viewer.
- `tests/` - CLI-level behavior tests.

## Engineering Rules

- Keep stdout output valid and scriptable.
- Keep interactive behavior behind TTY detection.
- Preserve JSON string values. The viewer may highlight embedded XML, but formatted output must not rewrite string contents.
- Update `README.md` when CLI flags, install steps, or user-visible behavior changes.
- Prefer Linux-first behavior, but avoid unnecessary non-portable code when portable Rust is simple.

## Verification Requirements

- Run `cargo fmt`.
- Run `cargo test`.
- Run `cargo clippy --all-targets -- -D warnings` when Clippy is available.
- For TUI changes, run the built CLI under a real PTY, for example with `script`, and verify scroll/quit keys.
