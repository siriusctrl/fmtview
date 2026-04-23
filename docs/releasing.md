# Releasing

This document is for maintainers and agents preparing a public release.

Do not move this workflow into `README.md`. README should describe installation
methods that are already available to users. Release mechanics, unpublished
distribution plans, tokens, and maintainer checklists belong here.

## Distribution Channels

Current stable channel:

- Git install: `cargo install --git https://github.com/siriusctrl/fmtview --locked`

Target public channels:

- GitHub Releases with prebuilt binaries and checksums.
- crates.io, so Rust users can run `cargo install fmtview --locked`.
- npm, so users without a Rust toolchain can run `npm install -g @siriusctrl/fmtview`.

Add a channel to README only after it is actually published and verified.

## Version Policy

- Keep the Cargo package version as the source of truth.
- Use semver.
- Release tags should be `vX.Y.Z`.
- The tag version must match `Cargo.toml` exactly.
- Do not publish from arbitrary branch pushes or pull requests.

Recommended release sequence:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
git status -sb
```

Then update `Cargo.toml`, commit, tag, and push:

```sh
git commit -am "chore: release vX.Y.Z"
git tag vX.Y.Z
git push origin main vX.Y.Z
```

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
- Run `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings`.
- Build release binaries in a target matrix.
- Package each binary as `fmtview-<target>.tar.gz`.
- Generate `sha256sums.txt`.
- Create or update a GitHub Release for the tag.
- Upload binaries and checksums as release assets.

Suggested initial target matrix:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

Add macOS targets before npm publication:

- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

## crates.io

Before the first crates.io publish, ensure `Cargo.toml` has complete package
metadata:

- `description`
- `license`
- `repository`
- `readme`
- `keywords`
- `categories`

Use `cargo publish --dry-run` before publishing.

Publish only from a release tag or an approved release workflow. If publishing
from CI, store the crates.io token in a protected GitHub environment.

After the first publish, README may list:

```sh
cargo install fmtview --locked
```

## npm

npm should be an installation wrapper for prebuilt Rust binaries, not a second
implementation.

Recommended package layout:

- `@siriusctrl/fmtview` - JS shim, command entrypoint, and optional platform
  dependencies.
- `@siriusctrl/fmtview-linux-x64` - Linux x64 binary package.
- `@siriusctrl/fmtview-linux-arm64` - Linux ARM64 binary package.
- `@siriusctrl/fmtview-darwin-x64` - macOS Intel binary package.
- `@siriusctrl/fmtview-darwin-arm64` - macOS Apple Silicon binary package.

The root npm package should expose the CLI through `package.json` `bin` and
select the installed platform package at runtime.

Prefer platform packages in `optionalDependencies` over downloading binaries in
`postinstall`. Platform packages are easier to audit and behave better in
restricted CI environments.

When publishing to npm from GitHub Actions:

- Use a protected `npm` environment.
- Use `actions/setup-node` with the npm registry URL.
- Publish public packages with provenance.
- Grant `id-token: write` only to the npm publish job.
- Keep npm publish jobs tag-gated and never run them for pull requests.

README may list npm installation only after every required platform package and
the root wrapper package have been published and smoke-tested:

```sh
npm install -g @siriusctrl/fmtview
```

## Release Checklist

Before tagging:

- Version updated in `Cargo.toml`.
- `cargo fmt --check` passes.
- `cargo test` passes.
- `cargo clippy --all-targets -- -D warnings` passes.
- `cargo build --release` passes.
- TUI smoke test runs under a real PTY if viewer behavior changed.
- README lists only installation channels that already work.

After publishing:

- Install from the published channel and run `fmtview --version`.
- Open a JSON, JSONL, and XML sample in the viewer.
- Verify GitHub Release checksums.
- Verify crates.io or npm package pages point to the repository.

If a bad release is published:

- Prefer publishing a fixed patch release.
- Use `cargo yank` only for crates that should no longer be selected by new
  dependency resolution.
- Deprecate or unpublish npm packages only when the npm policy and timing allow
  it; otherwise publish a fixed version.
