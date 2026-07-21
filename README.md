# auto-oss

A contribution protocol for **user-side coding agents**: when a user of an
open-source project hits a problem, their own agent turns that feedback into a
patch and submits it upstream — under conditions the repository has declared
in advance by publishing an `auto-oss.yml` policy file.

Existing automation (OpenHands Resolver, Copilot coding agent, Sentry Seer)
works for the repository's *owners*. auto-oss is the other direction: the
*outside user* is the starting point, and the protocol exists so that
agent-made contributions arrive already screened against conditions the
maintainer set, and mechanically identifiable as such — opt-in only,
quality-gated, disclosed, and always with a human as the author of record.
That's compliance, not a promise of good faith: `autos verify` checks that a
submission's claims are internally consistent and re-derives the ones it
practically can (diff size, from GitHub itself), but it cannot see into a
contributor's intent, and it does not execute the declared gates itself.

- [docs/SPEC.md](docs/SPEC.md) — protocol specification (v0 draft): the
  `auto-oss.yml` policy file and the submission metadata block
- [docs/CLI.md](docs/CLI.md) — `autos` command reference
- [docs/DESIGN.md](docs/DESIGN.md) — architecture and design rationale
- [docs/ja/](docs/ja/) — 日本語版ドキュメント (Japanese translations)
- [CHANGELOG.md](CHANGELOG.md) — release history

## Reference CLI: `autos`

Installing the `auto-oss` crate gives you the `autos` command:

```
autos policy <repo>    # show a repository's acceptance policy (or that it has none)
autos init             # generate an auto-oss.yml for your repository
autos fix <repo> "<feedback>" [--repro R] [--backend B] [--dry-run]
                       # feedback -> agent patch -> policy gates -> human review -> PR
autos feat / docs / refactor / test / typo ...
                       # same as `fix`, scope set by the verb (Conventional-Commits style)
autos verify <pr-url>  # check a PR's metadata block against the policy (CI-friendly)
autos status           # list recent and in-progress fix runs
autos resume <workdir> # pick an interrupted fix run back up
```

Each of `fix`/`feat`/`docs`/`refactor`/`test`/`typo` clones the target,
delegates patch generation to an agent backend (Claude Code by default),
runs the policy's gates locally, and only submits a pull request when
everything passes — otherwise it falls back to a structured issue or
discussion, as the policy directs. `fix` additionally takes `--scope` for a
scope your target declares that isn't one of these verbs. Submission always
happens from your own account, after you approve the final diff. Requires
`git`, `curl`, and `gh`; the Claude Code backend also needs `claude`, but the
human backend does not.

Status: v0.1 — protocol spec and working CLI, pre-announcement.

## Name

The *auto-* in auto-oss is not automation's *auto*. It is Greek **αὐτός** —
*self, of one's own will* — the same root that gave *automatos*, "acting of
its own accord". That is the point of the protocol: the person who feels the
problem acts on it, of their own will, through their own agent. The CLI
command `autos` carries the root directly.

## License

- The specification ([docs/SPEC.md](docs/SPEC.md)) is licensed under
  [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
- Everything else in this repository (including the reference CLI) is
  dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE),
  at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this repository by you shall be licensed as above
(spec changes under CC BY 4.0, code under MIT OR Apache-2.0), without any
additional terms or conditions.
