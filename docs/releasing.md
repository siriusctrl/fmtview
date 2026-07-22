# Releasing

This document is for maintainers and agents preparing a public release.

Do not move this workflow into `README.md`. README should describe installation
methods that are already available to users. Release mechanics, unpublished
distribution plans, tokens, and maintainer checklists belong here.

## Distribution Channels

Current stable channels:

- crates.io: `cargo install fmtview --locked`.
- crates.io library dependency: `fmtview-core` is published for the `fmtview`
  package, but end users should still install the `fmtview` binary.
- npm Linux x64 binary wrapper: `npm install -g fmtview`.
- GitHub Releases with a static Linux x64 binary archive and checksums.
- Git install: `cargo install --git https://github.com/siriusctrl/fmtview --locked`

Current binary coverage:

- `x86_64-unknown-linux-musl` for GitHub Release artifacts and npm.

Potential future channels:

- Linux aarch64.
- macOS x64 and Apple Silicon.

Add a channel to README only after it is actually published and verified.

## Version Policy

- Keep the root Cargo package version as the source of truth.
- Keep `crates/fmtview-core/Cargo.toml` at the same version as the root package.
- Use semver.
- Release tags should be `vX.Y.Z`.
- The tag version must match `Cargo.toml` exactly.
- Do not publish from arbitrary branch pushes or pull requests.

Recommended release sequence:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release --locked
cargo publish -p fmtview-core --dry-run --locked
git status -sb
```

If registry secrets changed, run the manual `Release Auth Check` workflow before
tagging. That workflow checks that the crates.io secret is configured, runs a
crates.io package dry-run, and validates the npm package manifest. The crates.io
token itself is fully validated only by the real publish step because Cargo
dry-runs do not upload.

Then update the root `Cargo.toml`, `crates/fmtview-core/Cargo.toml`, and
`npm/fmtview/package.json` together, commit, tag, and push:

```sh
git commit -am "chore: release vX.Y.Z"
git tag vX.Y.Z
git push origin main vX.Y.Z
```

## Changelog And Release Notes

`CHANGELOG.md` is the source of truth for user-facing release notes. Every
release must add a `## [X.Y.Z] - YYYY-MM-DD` section before the tag is pushed.

Use `## [Unreleased]` while changes are accumulating. Before tagging, move the
relevant entries into the versioned section and keep the release notes concise:

- Mention user-visible behavior changes.
- Mention packaging, install, and compatibility changes.
- Mention breaking changes or behavior tradeoffs explicitly.
- Do not include internal-only refactors unless they affect users or
  maintainers.

The GitHub Release workflow extracts the matching `CHANGELOG.md` section for
the tag. If the section is missing or empty, the release job fails instead of
publishing placeholder notes.

## GitHub Actions Release Shape

Use a dedicated release workflow triggered by version tags:

```yaml
on:
  push:
    tags:
      - "v*"
```

The workflow should:

- Check out the tagged commit.
- Verify the tag version matches `Cargo.toml`.
- Verify the `fmtview-core` and npm package versions match the root `Cargo.toml`.
- Verify `CHANGELOG.md` has a non-empty section for the tag.
- Run `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings`.
- Build the Linux x64 release binary as a static musl binary.
- Package the binary as `fmtview-linux-x64.tar.gz`.
- Generate `sha256sums.txt`.
- Create or update a GitHub Release for the tag.
- Upload the binary archive and checksums as release assets.
- Publish `fmtview-core` to crates.io first, wait until that exact version is
  available, then publish `fmtview` if `CARGO_REGISTRY_TOKEN` is configured.
- Publish to npm using a publish-capable `NPM_TOKEN`, or through Trusted
  Publishing after it is configured.
- Support manual reruns for an existing tag and skip registry versions that are
  already published.

If the crates.io or npm secret is missing, the workflow still builds the GitHub
Release artifact and skips that registry publish step.

Initial target:

- `x86_64-unknown-linux-musl`

The Linux x64 artifact is intentionally built with musl static linking. This is
the default for GitHub Release and npm Linux x64 binaries so npm installs do not
inherit the GitHub Actions runner's glibc version requirement.

Potential future targets:

- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

## crates.io

Keep `Cargo.toml` package metadata complete:

- `description`
- `license`
- `repository`
- `readme`
- `keywords`
- `categories`

Use `cargo publish -p fmtview-core --dry-run --locked` before publishing. The
root `fmtview` dry-run can resolve a release version only after the matching
`fmtview-core` version exists on crates.io.

The two crates are released from the same tag and use the same version, but the
registry order is strict:

```sh
cargo publish -p fmtview-core --locked
# Wait for fmtview-core X.Y.Z to become available from crates.io.
cargo publish -p fmtview --dry-run --locked
cargo publish -p fmtview --locked
```

Do not publish the root package with a path-only or mismatched core dependency.
Its manifest must keep both `version = "X.Y.Z"` and
`path = "crates/fmtview-core"`: workspace builds use the path, while the
packaged crate resolves the published version.

Publish only from a release tag or an approved release workflow. The release
workflow uses the `CARGO_REGISTRY_TOKEN` GitHub secret when it is configured:

```sh
gh secret set CARGO_REGISTRY_TOKEN
```

## npm

npm should be an installation wrapper for prebuilt Rust binaries, not a second
implementation.

Current package layout:

- `fmtview` - JS shim plus a bundled static Linux x64 binary.

The npm package exposes the CLI through `package.json` `bin`, and the shim
executes `vendor/fmtview`. It is currently Linux x64 only. The bundled binary
should come from the `x86_64-unknown-linux-musl` release artifact, not from the
host `x86_64-unknown-linux-gnu` target.

A future multi-platform npm release should move to a root wrapper package plus
platform packages in `optionalDependencies`. Prefer that over downloading
binaries in `postinstall`; platform packages are easier to audit and behave
better in restricted CI environments.

When publishing to npm from GitHub Actions with a token:

- Use `actions/setup-node` with the npm registry URL.
- Publish public packages with provenance.
- Grant `id-token: write` only to the npm publish job.
- Keep npm publish jobs tag-gated and never run them for pull requests.
- Configure an npm granular access token that can publish `fmtview` and bypass
  2FA for automation, then store it as `NPM_TOKEN`:

```sh
gh secret set NPM_TOKEN
```

Trusted Publishing is preferred once the npm package exists:

- Configure the trusted publisher for package `fmtview` with GitHub owner
  `siriusctrl`, repository `fmtview`, and workflow filename `release.yml`.

```sh
npm trust github fmtview --repo siriusctrl/fmtview --file release.yml --yes
```

If npm returns `Two-factor authentication is required for this operation`, pass
the current one-time password locally:

```sh
npm trust github fmtview --repo siriusctrl/fmtview --file release.yml --yes --otp <code>
```

The equivalent npmjs.com setup is package settings -> Trusted Publishers ->
GitHub Actions, with the same owner, repository, and workflow filename. npm CLI
automatically detects the GitHub Actions OIDC environment during
`npm publish --provenance --access public`.

## Release Checklist

Before tagging:

- Version updated in the root `Cargo.toml`.
- Matching version updated in `crates/fmtview-core/Cargo.toml`.
- Version updated in `npm/fmtview/package.json`.
- `CHANGELOG.md` has a section for the release version.
- `cargo fmt --check` passes.
- `cargo test` passes.
- `cargo clippy --all-targets -- -D warnings` passes.
- `cargo build --release --locked` passes.
- TUI smoke test runs under a real PTY if viewer behavior changed.
- Real-terminal visual smoke recording runs with Xvfb/Kitty/ffmpeg if viewer
  layout, color, navigation, search, gutter, or diff UI behavior changed:
  `scripts/record-emulator-demo.sh target/fmtview-emulator-recordings/<name> -- target/release/fmtview examples/chat.jsonl`.
- README lists only installation channels that already work.

After publishing:

- Install from crates.io and npm when those channels are part of the release,
  then run `fmtview --version`.
- Run a small CLI smoke from the published package, for example
  `fmtview alias bash`.
- Open JSON, JSONL, XML-compatible markup, and HTML samples in the viewer.
- Verify GitHub Release checksums.
- Verify crates.io or npm package pages point to the repository.

If a bad release is published:

- Prefer publishing a fixed patch release.
- Use `cargo yank` only for crates that should no longer be selected by new
  dependency resolution.
- Deprecate or unpublish npm packages only when the npm policy and timing allow
  it; otherwise publish a fixed version.
