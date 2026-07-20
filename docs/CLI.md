English | [日本語](ja/CLI.md)

# `autos` CLI Reference

The reference client for the [auto-oss protocol](SPEC.md).

```
cargo install auto-oss   # installs the `autos` binary
```

External requirements: `git`, `curl`, and an authenticated `gh` (GitHub CLI).
The `claude-code` backend additionally needs the `claude` command; the
`human` backend needs nothing else.

## Commands

### `autos policy <repo>`

Show a repository's acceptance policy, or state that it has none.

`<repo>` accepts a local path, `owner/repo`, or a GitHub URL (all commands
that take `<repo>` accept the same forms). The policy file is discovered per
SPEC §1: `auto-oss.yml` at the root first, then `.github/auto-oss.yml`.

Three outcomes are distinguished:

- **Opted in** — the policy is printed: scopes, gates, diff limit,
  requirements, fallback, label.
- **Not opted in** — no policy file. The protocol forbids submitting
  agent-generated pull requests to this repository, and the output says so.
- **Unusable** — a policy file exists but does not parse, or declares an
  unsupported spec version. Treated as not opted in, with the reason shown.

A repository that cannot be reached is an error, never "not opted in".

### `autos fix <repo> "<feedback>" [options]`

The main pipeline: turn feedback into a policy-gated submission.

| Option | Default | Meaning |
|---|---|---|
| `--scope <s>` | `bug-fix` | Change category; must be listed in the policy's `accepts.scopes` |
| `--repro <text>` | — | Reproduction steps; required by policies with `require.reproduction` for bug fixes |
| `--backend <b>` | `claude-code` | Patch producer: `claude-code` or `human` |
| `--dry-run` | off | Stop after gates and preview; submit nothing |

The pipeline, in order:

1. **Policy discovery.** No opt-in → the command refuses and stops. The
   requested scope is validated against the policy before any work happens.
2. **Clone** into a fresh temporary work directory.
3. **Patch generation** by the backend. `claude-code` runs Claude Code
   non-interactively with the feedback, scope, and size limit injected as
   constraints. `human` prints the constraints and waits while you edit the
   work directory yourself.
4. **Size check.** A diff exceeding `accepts.max_diff_lines` is downgraded
   to the policy's fallback.
5. **Gates.** Every command declared under `gates.*` runs in the clone.
   Output streams to your terminal.
6. **Preview and confirmation.** The full diff, gate results, and the exact
   submission body (with its metadata block) are shown; nothing is submitted
   without your explicit `y`. With `--dry-run` the command stops here.
7. **Submission.** If you have push access to the target, the branch is
   pushed to the repository itself; otherwise a fork is created and the pull
   request is opened cross-repository. Either way the submission comes from
   **your** account, with the SPEC §3 metadata block embedded, and the
   policy's label applied best-effort.
8. **Fallback.** If gates failed or the diff was oversized, the collected
   context (and the partial diff) is submitted as a structured issue instead
   — when the policy's `fallback` says so, and again only after confirmation.

Local repositories run the same pipeline but stop before submission.

Declared `limits.per_author_per_week` are self-enforced, as SPEC §4 asks:
submissions are logged locally in `~/.auto-oss/submissions.tsv`, and `fix`
refuses to start when the rolling seven-day count for the target repository
has reached the limit.

### `autos init [--force]`

Maintainer side: interactively generate an `auto-oss.yml` for the current
directory. Prompts for scopes, gates, diff limit, reproduction requirement,
and fallback. The result is round-tripped through the policy parser before
being written, so an invalid policy is never produced. `--force` overwrites
an existing file.

### `autos verify <pr-url>`

Maintainer/CI side: fetch a pull request, extract its metadata block, and
check it against the repository's policy. Verified: exactly one block,
accepted scope, non-empty feedback and backend disclosure, `human_reviewed`
when required, reproduction steps when required, and a `pass` report for
every declared gate.

A PR with no metadata block is an ordinary contribution and passes
trivially. Violations are listed and the exit code is non-zero, so the
command drops straight into CI:

```yaml
- run: cargo run --quiet -- verify "${{ github.event.pull_request.html_url }}"
  env:
    GH_TOKEN: ${{ github.token }}
```

Note that `verify` checks conformance of the *claims*; re-running the gates
(as this repository's CI does) is what checks their truth.
