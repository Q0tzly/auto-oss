# Contributing

Thanks for your interest. A few ground rules keep this project coherent:

## Spec changes (SPEC.md)

The spec is the product. Open an issue describing the problem **before**
sending a PR that changes SPEC.md — wording fixes excepted. Incompatible
forks of the spec must not use the auto-oss name (see SPEC.md §6).

## DESIGN.md

[DESIGN.md](DESIGN.md) records the project's architecture and the reasoning
behind its decisions, as judged by the maintainer. It is not a collaborative
document: PRs against it are generally not accepted, apart from typo fixes
and translation upkeep under `docs/ja/`. If you disagree with a recorded
decision, open an issue — the document gets a new entry rather than a
rewrite.

## Code (the `autos` CLI)

Ordinary contributions are welcome. `cargo test` and
`cargo clippy --all-targets -- -D warnings` must pass.

This repository itself accepts submissions under the auto-oss protocol — see
[auto-oss.yml](../auto-oss.yml) for the policy. Trying `autos fix` on this repo
is encouraged; that is what dogfooding means.

## Branches

Day-to-day development happens on `dev` — commit there as work lands, no
need to touch `main` in between. `main` only moves at release time: `dev`
is merged into it as part of the release steps below, so `main` always
points at a tagged (or about-to-be-tagged) release.

## Releases

Releases are tag-driven. Before creating a release tag:

1. On `dev`: move the release's entries from `Unreleased` into a versioned
   section in [CHANGELOG.md](../CHANGELOG.md), using
   `## [x.y.z] - YYYY-MM-DD`.
2. Update the comparison links at the bottom of the changelog.
3. Bump the version in `Cargo.toml` and refresh `Cargo.lock`.
4. Commit those changes on `dev` and confirm CI passes.
5. Merge `dev` into `main` (fast-forward) and push `main`.
6. Only then create and push the matching `vx.y.z` tag.

The release workflow refuses to publish when the tag, crate version, and
changelog entry do not agree.

## Licensing

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this repository by you shall be licensed as stated
in the README (spec changes under CC BY 4.0, code under MIT OR Apache-2.0),
without any additional terms or conditions.
