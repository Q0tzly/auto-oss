# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Versions 0.1.0 and 0.1.1 predate tag-driven releases; their links use the
corresponding version-boundary commits.

## [Unreleased]

### Added

- Added `feat`, `docs`, `refactor`, `test`, and `typo` as scope-shortcut
  subcommands alongside `fix`, named after Conventional Commits prefixes, so
  the common cases don't need `--scope`. `fix` keeps `--scope` as the
  escape hatch for a scope outside this set.
- Added `autos resume <workdir>` to pick an interrupted `fix` run back up
  (Ctrl-C, a closed terminal, a crash) without re-cloning or re-running the
  backend. `autos status` now prints the exact command for any resumable
  run.
- Implemented the `discussion` fallback via the GitHub GraphQL API, picking a
  discussion category by preference order and reporting clearly when a
  repository has none.
- Added a Japanese translation of `SECURITY.md`.

### Changed

- Include the `auto-oss` client version in pull-request and fallback issue
  bodies.
- Routed a backend failure or a no-op patch through the same policy
  `fallback` as an oversized diff or a failing gate, instead of aborting
  with a hard error and nothing to show for the run.
- Reworded claims of contributions being "trustworthy" to describe what is
  actually checkable: compliance with a repository's declared policy, with
  `autos verify` re-deriving diff size independently rather than trusting
  the submission's own numbers. Gate-pass and human-review claims remain
  self-reported without the repository's own CI re-running them.

## [0.1.5] - 2026-07-21

### Added

- Added `autos status` to inspect recent and currently running `fix` jobs.
- Added configurable custom backends through `~/.auto-oss/config.yml`.
- Streamed Claude Code progress and included backend-provided change summaries
  and titles in submissions.
- Added network-free CLI integration fixtures using a fake home directory,
  local git repositories, and a deterministic custom backend.
- Added optional model configuration for Claude Code and custom backends, with
  the configured model disclosed in submission metadata.
- Added `metadata.language` so repositories can request a language for
  submission titles and summaries while retaining feedback verbatim.

### Changed

- Quoted the original feedback explicitly in submission bodies.
- Required confirmation immediately before executing repository-declared
  gates, with the wait and abort states recorded by the run tracker.
- Made GitHub policy-fetch failures explain the likely network or repository
  problem instead of surfacing only the underlying `curl` error.
- Made parallel `fix` runs use distinct work directories, branch names, and
  status files, and append safely to the submission log.
- Expanded this repository's accepted scopes to include `feature` and
  `refactor`, with a 300-line limit.

### Fixed

- Made `autos verify` enforce the policy's actual changed-line limit using the
  pull request's additions and deletions.
- Rejected blank reproduction text when a policy requires reproduction steps.

## [0.1.3] - 2026-07-21

### Added

- Added a full English CLI reference and design document, plus Japanese
  translations of the specification, design, and CLI documentation.
- Added local enforcement of `limits.per_author_per_week` using a rolling
  submission log under `~/.auto-oss/`.

### Changed

- Moved the specification, design, contribution, and security documents under
  `docs/` and refreshed the project overview.

### Fixed

- Rejected empty feedback before patch generation.
- Made labels best-effort for issue fallbacks, matching pull-request behavior.

## [0.1.2] - 2026-07-20

### Added

- Added `autos verify` and a receiving-side CI conformance check for auto-oss
  metadata blocks.
- Added CI for formatting, build, tests, and Clippy.
- Added tag-driven crates.io publishing with Trusted Publishing and documented
  the contributor-side threat model.

### Changed

- Pushed branches directly to upstream when the contributor has access and
  used a fork only for outside contributions.

### Fixed

- Detached backend standard input so Claude Code cannot consume confirmation
  input intended for the user.
- Documented all external CLI dependencies.

### Security

- Pinned GitHub Actions to full commit SHAs.

## [0.1.1] - 2026-07-20

### Added

- Added the `human` backend so hand-written patches use the same policy,
  gates, metadata, and submission pipeline.
- Added contribution guidance and enabled bug-fix dogfooding in this
  repository's own policy.

### Changed

- Renamed the installed command from `auto-oss` to `autos` while retaining
  `auto-oss` as the crate and protocol name.

## [0.1.0] - 2026-07-20

### Added

- Published the v0 draft protocol, including repository opt-in policy and
  machine-readable submission metadata formats.
- Added the initial Rust reference CLI with `policy`, `init`, and `fix`
  commands.
- Added Claude Code patch generation, local policy gates, diff-size limits,
  human confirmation, pull-request submission, and structured issue fallback.
- Established CC BY 4.0 licensing for the specification and dual MIT or
  Apache-2.0 licensing for the implementation.

[Unreleased]: https://github.com/q0tzly/auto-oss/compare/v0.1.5...HEAD
[0.1.5]: https://github.com/q0tzly/auto-oss/compare/v0.1.3...v0.1.5
[0.1.3]: https://github.com/q0tzly/auto-oss/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/q0tzly/auto-oss/compare/80cd0fb...v0.1.2
[0.1.1]: https://github.com/q0tzly/auto-oss/compare/7868337...80cd0fb
[0.1.0]: https://github.com/q0tzly/auto-oss/tree/7868337
