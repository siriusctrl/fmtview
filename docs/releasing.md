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
- npm, so Linux x64 users without a Rust toolchain can run `npm install -g fmtview`.

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

If registry secrets changed, run the manual `Release Auth Check` workflow before
tagging. That workflow verifies the npm token with `npm whoami`, checks that
the crates.io secret is configured, and runs a crates.io package dry-run. The
crates.io token itself is fully validated only by the real publish step because
Cargo dry-runs do not upload.

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
- Verify the npm package version matches `Cargo.toml`.
- Run `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings`.
- Build the Linux x64 release binary.
- Package the binary as `fmtview-linux-x64.tar.gz`.
- Generate `sha256sums.txt`.
- Create or update a GitHub Release for the tag.
- Upload the binary archive and checksums as release assets.
- Publish to crates.io if `CARGO_REGISTRY_TOKEN` is configured.
- Publish to npm if `NPM_TOKEN` is configured.
- Support manual reruns for an existing tag and skip registry versions that are
  already published.

If either registry secret is missing, the workflow still builds the GitHub
Release artifact and skips that registry publish step.

Initial target:

- `x86_64-unknown-linux-gnu`

Potential future targets:

- `aarch64-unknown-linux-gnu`
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

Publish only from a release tag or an approved release workflow. The release
workflow uses the `CARGO_REGISTRY_TOKEN` GitHub secret when it is configured:

```sh
gh secret set CARGO_REGISTRY_TOKEN
```

After the first publish, README may list:

```sh
cargo install fmtview --locked
```

## npm

npm should be an installation wrapper for prebuilt Rust binaries, not a second
implementation.

Initial package layout:

- `fmtview` - JS shim plus a bundled Linux x64 binary.

The npm package exposes the CLI through `package.json` `bin`, and the shim
executes `vendor/fmtview`. It is intentionally Linux x64 only for the first
release.

A future multi-platform npm release should move to a root wrapper package plus
platform packages in `optionalDependencies`. Prefer that over downloading
binaries in `postinstall`; platform packages are easier to audit and behave
better in restricted CI environments.

When publishing to npm from GitHub Actions:

- Use `actions/setup-node` with the npm registry URL.
- Publish public packages with provenance.
- Grant `id-token: write` only to the npm publish job.
- Keep npm publish jobs tag-gated and never run them for pull requests.
- Configure the `NPM_TOKEN` GitHub secret:

```sh
gh secret set NPM_TOKEN
```

The npm token must be able to publish from CI. If the npm account has 2FA
enabled, use a granular access token that can bypass 2FA for publishing this
package; a token that only passes `npm whoami` is not enough.

README may list npm installation only after the package has been published and
smoke-tested:

```sh
npm install -g fmtview
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
