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
| `--scope <s>` | `bug-fix` | Change category; must be listed in the policy's `accepts.scopes`. Beyond `bug-fix`: `docs`, `typo`, `test`, `refactor`, and `feature` — use `feature` to propose an enhancement rather than fix a defect |
| `--repro <text>` | — | Reproduction steps; required by policies with `require.reproduction` for bug fixes |
| `--backend <b>` | config `default_backend`, else `claude-code` | Patch producer: `claude-code`, `human`, or a custom backend from the config |
| `--dry-run` | off | Stop after gates and preview; submit nothing |

The pipeline, in order:

1. **Policy discovery.** No opt-in → the command refuses and stops. The
   requested scope is validated against the policy before any work happens.
2. **Clone** into a fresh temporary work directory.
3. **Patch generation** by the backend. `claude-code` runs Claude Code with
   the feedback, scope, and size limit injected as constraints, streaming
   its progress (tool calls, commentary) to your terminal as it works.
   `human` prints the constraints and waits while you edit the work
   directory yourself. The backend also proposes the submission's **title**
   (`human` asks you for one) and its account of what changed goes in the
   body under "What changed"; your original feedback is quoted verbatim
   under "Original feedback". When the backend offers no title, the first
   line of the feedback is used, truncated. Titles are always prefixed with
   the scope.
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

### `autos status`

List recent `fix` runs — including ones still running in another terminal —
with their current phase (`cloning`, `generating`, `gates`,
`awaiting-approval`, `submitted-pr`, …). Run files live in
`~/.auto-oss/runs/` and are pruned after seven days.

### Configuration: `~/.auto-oss/config.yml`

```yaml
default_backend: claude-code

claude_code:
  model: claude-sonnet-5   # passed to `claude --model`; omit to let it choose

backends:
  codex:
    command: ["codex", "exec", "{prompt}"]
    model: gpt-5-codex     # disclosed in metadata; not passed to the command
```

Custom backends are arbitrary commands run inside the clone with `{prompt}`
substituted; they are expected to edit files and exit 0. Keep `{prompt}` as
its own argv element — interpolating it into a shell string breaks on the
prompt's newlines (and is a quoting hazard).

A configured model is recorded as `agent.model` in the submission metadata,
so maintainers can see what produced a patch. For custom backends the field
is disclosure only — if the tool needs a flag, put it in `command`.

### Running several fixes at once

`fix` runs are independent: each clones into its own work directory, writes
its own status file, and pushes its own branch (names carry a timestamp and
pid). You can run one per terminal against different repositories, or
against the same one. Two caveats: a repository's `limits.per_author_per_week`
counts submissions across all of them, and each run wants your terminal for
its approval prompt — so keep them in separate terminals.

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
