English | [日本語](ja/DESIGN.md)

# auto-oss Design

This document records the architecture of auto-oss and the reasoning behind
its decisions. It is descriptive; the normative protocol is [SPEC.md](SPEC.md).

## Problem

Coding agents made patches cheap, and maintainers are paying for it:
unsolicited AI-generated pull requests and bug reports have become a
recognized burden on open-source projects. Meanwhile, every existing
automation of the issue-to-PR loop — OpenHands Resolver, GitHub Copilot's
coding agent, Sentry Seer — is **owner-side**: the repository's own team
wires an agent into their own backlog.

Nobody had designed the other direction: the *outside user* who actually
hits a problem has an agent too. auto-oss exists so that user's agent can
turn feedback into a patch and deliver it upstream **already screened
against conditions the maintainer set, and mechanically identifiable as
such**. That is a weaker claim than "trustworthy" on purpose: compliance is
checkable (`autos verify` re-derives what it can — diff size, from GitHub
itself — instead of taking the submission's word for it), but nothing here
verifies a contributor's good faith, and gate results themselves are still
self-reported. See [Security model](#security-model).

## Position: a protocol, not a tool

The hard problem is not producing a patch — agents do that. The hard problem
is that an unsolicited agent PR is indistinguishable from spam. So the core
of auto-oss is a contract, fixed as file formats:

1. **Maintainer opt-in** — only repositories that publish `auto-oss.yml`
   receive submissions. The absence of the file is a refusal that conforming
   clients must honor.
2. **Quality gates** — the policy declares the commands (test, lint, build)
   a patch must pass. Failing patches are downgraded to structured issues,
   never submitted as PRs.
3. **Disclosure and accountability** — every submission embeds a
   machine-readable provenance block (original feedback, backend, gate
   results), and a human remains the author of record.

Patch generation is deliberately delegated to existing agents (Claude Code
today; others pluggable). The value of auto-oss is the contract, not another
agent — and the contract also works in reverse: publishing a policy is
simultaneously a statement of what a repository will *not* accept.

## Architecture

```
user                            autos CLI                          upstream repository
 |                                 |                                    |
 | "this behavior is wrong"        |                                    |
 |-------------------------------->|                                    |
 |                                 |--- fetch policy (auto-oss.yml) --->|
 |                                 |<-- acceptance conditions ----------|
 |                                 |                                    |
 |                                 |-- clone into a fresh workdir       |
 |                                 |-- agent backend generates patch    |
 |                                 |-- run declared gates (test/lint)   |
 |                                 |                                    |
 |<-- diff + gate results ---------|                                    |
 | approve                         |                                    |
 |-------------------------------->|-- push branch, open PR ----------->|
 |                                 |   (direct with access, else fork)  |
 |                                 |                                    |
 |                                 |   gates failed -> structured issue
```

Module layout of the reference implementation (Rust, single binary):

| Module | Responsibility | SPEC section |
|---|---|---|
| `policy` | discovery, parsing, defaults, repo references | §1–2 |
| `fix` | the submission pipeline | §4 |
| `backend` | agent abstraction (`claude-code`, `human`) | — |
| `gates` | gate execution | §2 |
| `metadata` | metadata block rendering | §3 |
| `verify` | receiving-side conformance checking | §3–5 |

## Decisions

### Policy file at the repository root

Discovery order is `auto-oss.yml`, then `.github/auto-oss.yml`. The root
location is recommended: opting in is a contract, and contracts benefit from
being visible (`FUNDING.yml` hides in `.github/` because it is metadata;
this is not).

### Metadata as YAML inside an HTML comment

The submission block lives in the PR/issue body as `<!-- auto-oss:v0 ... -->`.
Invisible to human readers, machine-readable to maintainers' tooling, and —
critically — portable: it depends on no forge-specific feature, so the
protocol works anywhere git and pull requests exist.

### Pluggable backends; the client never calls an LLM

`autos` assembles the prompt (feedback + policy constraints) and delegates to
a backend subprocess. This keeps the client small, keeps model choice with
the user, and means every improvement in coding agents is inherited for
free. The reserved backend `human` exists because disclosure must be
truthful: a hand-written patch declares itself as such rather than
pretending an agent was involved — the machinery (opt-in, gates, provenance)
applies all the same.

### Human confirmation before submission

The CLI always shows the final diff, gate results, and submission body, and
waits for explicit approval. The policy-level `require.human_review` maps to
an attestation (`human_reviewed`) in the metadata. The attestation cannot be
technically verified in v0 — its value is contractual: a false attestation
is documented grounds for rejection and banning.

### Downgrade instead of forcing a PR

When gates fail or the diff exceeds the declared limit, the run is not
discarded: reproduction steps, environment, and the partial diff are still
valuable, so they are submitted as a structured issue if the policy's
`fallback` allows. "A PR only when quality is proven" is the anti-spam core.

### Scope verbs, not just a `--scope` flag

`fix`, `feat`, `docs`, `refactor`, `test`, and `typo` are separate
subcommands, not one command with a required flag — named after
[Conventional Commits](https://www.conventionalcommits.org/) prefixes,
which most of the target audience already reaches for. Each is a thin
wrapper that fixes the scope and otherwise runs the identical pipeline.
`fix` alone keeps a `--scope` flag as the escape hatch for a repository
that declares a scope outside this well-known set — `accepts.scopes` is a
free-form list, so the protocol can't assume these six cover every policy.

### Direct push with access, fork without

A pull request needs its head branch hosted on the forge. Contributors with
push access branch on the upstream repository itself; outsiders go through a
fork — the only route GitHub offers them. Both paths were validated against
real repositories before release.

### Rust, single binary

The client runs on contributors' machines; a dependency-light single binary
distributes well. Since patch generation is delegated, the client itself is
orchestration and I/O only.

### Naming: the protocol is auto-oss, the command is autos

`autos` is Greek **αὐτός** — *self, of one's own will* — which is the point:
the person who feels the problem acts on it through their own agent. As a
project name, "autos" would drown in automobile search results, so the
searchable name stays `auto-oss` and the root survives in the binary
(the ripgrep/`rg` convention).

## Security model

The protocol protects maintainers from unwanted submissions; the reverse
direction is documented in [SECURITY.md](SECURITY.md): policy gates execute
on the *contributor's* machine, so running `autos fix` (or any of its
scope-shortcut siblings — `feat`, `docs`, `refactor`, `test`, `typo`, all
thin wrappers over the same pipeline) against a repository means trusting
that repository — and repository contents are untrusted input (prompt
injection) to the backend agent. Sandboxed gate execution is a v1
candidate.

## Future (SPEC v1 candidates)

- `accepts.paths` — path allow/deny lists. Realized after going public:
  a `docs` scope exposes even a project's decision log to rewrites.
- Sandboxed gate execution, on both sides: the contributor's machine runs
  the target's declared commands (see Security model), and a repository
  that wants to re-run gates server-side to check claims has no isolated
  place to do it either.
- Additional backends (OpenHands, OpenCode).
- Receiving-side rate-limit enforcement — `limits.per_author_per_week` is
  self-enforced by the client today (SPEC's SHOULD), with no check on the
  repository side that a client is being honest about it.
- Signatures or reputation for metadata claims that can't be independently
  re-derived (`human_reviewed`, gate-pass results). `autos verify` already
  re-derives diff size directly from GitHub rather than trusting the
  submission's own number; gate-pass and review claims have no such check
  short of the repository's own CI re-running the same commands.
- Forges beyond GitHub (GitLab, Codeberg) — the SPEC is already
  forge-agnostic; only the client binds to `gh`.

## Standing risks

- **Platform absorption.** GitHub or another forge may ship first-party
  "external agent PR" controls. Being first with an open, forge-agnostic
  de-facto contract is the defense — absorption of an open standard is a
  form of winning.
- **False metadata.** `human_reviewed` and gate-pass claims can be forged;
  `autos verify` checks form and independently re-derives diff size, but it
  does not execute the declared gates — a repository wiring its own CI to
  run the same commands its policy declares (as this repository's `ci.yml`
  does) is the real check. A repository that opts in without doing that is
  trusting the gate claims outright.
- **Cold start.** A protocol with no opted-in repositories is a spec sheet.
  Mitigations: dogfooding from day one, and pitching maintainers who are
  publicly drowning in agent spam — for whom the policy file doubles as a
  fence.
