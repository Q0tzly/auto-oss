# Contributing

Thanks for your interest. A few ground rules keep this project coherent:

## Spec changes (SPEC.md)

The spec is the product. Open an issue describing the problem **before**
sending a PR that changes SPEC.md — wording fixes excepted. Incompatible
forks of the spec must not use the auto-oss name (see SPEC.md §6).

## DESIGN.md

DESIGN.md is the maintainer's decision log, written in Japanese. It records
*why* choices were made at the time they were made. It is not a collaborative
document: PRs against it are generally not accepted, apart from typo fixes.
If you disagree with a recorded decision, open an issue — the log gets a new
entry rather than a rewrite.

## Code (the `autos` CLI)

Ordinary contributions are welcome. `cargo test` and
`cargo clippy --all-targets -- -D warnings` must pass.

This repository itself accepts submissions under the auto-oss protocol — see
[auto-oss.yml](../auto-oss.yml) for the policy. Trying `autos fix` on this repo
is encouraged; that is what dogfooding means.

## Licensing

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this repository by you shall be licensed as stated
in the README (spec changes under CC BY 4.0, code under MIT OR Apache-2.0),
without any additional terms or conditions.
