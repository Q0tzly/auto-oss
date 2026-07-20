English | [日本語](ja/SPEC.md)

# auto-oss Protocol Specification — v0 (draft)

auto-oss is a contribution protocol for **user-side coding agents**: an agent
acting on behalf of a project's *user* (not its maintainers) turns that user's
feedback into a patch and submits it upstream, under conditions the repository
has declared in advance.

The protocol has two artifacts:

1. **`auto-oss.yml`** — a policy file a repository publishes to opt in and
   declare its acceptance conditions.
2. **The submission metadata block** — a machine-readable block embedded in
   every pull request or issue produced under this protocol.

The protocol is forge-agnostic: it assumes only git and the existence of
patches/pull requests and issues. Nothing in this spec depends on GitHub.

The key words MUST, MUST NOT, SHOULD, and MAY are to be interpreted as
described in RFC 2119.

---

## 1. Opt-in and discovery

A repository opts in by publishing a policy file. Clients MUST look for it in
this order and use the first one found:

1. `auto-oss.yml` (repository root)
2. `.github/auto-oss.yml`

If no policy file exists, the repository has **not** opted in. Clients
MUST NOT open agent-generated pull requests against repositories that have not
opted in. Clients MAY still help the user write an ordinary issue or report,
but MUST NOT mark it as an auto-oss submission.

## 2. Policy file: `auto-oss.yml`

```yaml
# Minimal example
version: 0

accepts:
  scopes: [bug-fix, docs, typo]

gates:
  test: "cargo test"
```

```yaml
# Full example
version: 0

accepts:
  # Change categories this repository accepts from user-side agents.
  # Well-known values: bug-fix, docs, typo, test, refactor, feature
  scopes: [bug-fix, docs, typo]
  # Reject patches larger than this many changed lines (added + removed).
  max_diff_lines: 300

gates:
  # Commands run from the repository root. All declared gates MUST pass
  # (exit code 0) for a submission to qualify as a pull request.
  build: "cargo build"
  test: "cargo test"
  lint: "cargo clippy -- -D warnings"

require:
  # The submitting human confirms they reviewed the patch before submission.
  human_review: true
  # Feedback must include reproduction steps (for bug-fix scope).
  reproduction: true

# What the client should do when a patch cannot be produced or gates fail:
#   issue      - submit collected context as a structured issue (default)
#   discussion - submit to the forge's discussion feature, if any
#   none       - do not submit anything
fallback: issue

limits:
  # Advisory in v0: clients SHOULD self-enforce, maintainers MAY enforce.
  per_author_per_week: 3

metadata:
  # Label the client SHOULD apply to submissions, if the forge supports labels.
  label: "auto-oss"
```

### 2.1 Field semantics

| Field | Required | Meaning |
|---|---|---|
| `version` | yes | Spec major version. This document defines `0`. |
| `accepts.scopes` | yes | Allowed change categories. A client MUST classify its patch into exactly one scope and MUST NOT submit a PR whose scope is not listed. |
| `accepts.max_diff_lines` | no | Upper bound on patch size. Exceeding it downgrades the submission to `fallback`. |
| `gates.*` | no | Named shell commands. Every declared gate MUST exit 0 for PR submission. Gate names are free-form; `build`, `test`, `lint` are conventional. |
| `require.human_review` | no (default `true`) | The client MUST have a human review the final diff and attest to it in the metadata block. |
| `require.reproduction` | no (default `false`) | Bug-fix submissions MUST include reproduction steps. |
| `fallback` | no (default `issue`) | Behavior when the patch pipeline fails. |
| `limits.per_author_per_week` | no | Advisory rate limit per submitting human. |
| `metadata.label` | no | Label for submissions. |

A policy file that fails to parse MUST be treated as absent (no opt-in).
Unknown fields MUST be ignored (forward compatibility).

## 3. Submission metadata block

Every pull request or issue submitted under this protocol MUST embed exactly
one metadata block in its body, as an HTML comment containing YAML:

```markdown
<!-- auto-oss:v0
scope: bug-fix
feedback: |
  Verbatim user feedback that motivated this change.
reproduction: |
  1. Run `foo --bar`
  2. Observe panic
environment:
  os: macOS 15.2
  version: foo 1.4.2
agent:
  backend: claude-code
  model: claude-fable-5
gates:
  build: pass
  test: pass
  lint: pass
human_reviewed: true
client: auto-oss/0.1.0
-->
```

Requirements:

- `scope` MUST be one of the repository's `accepts.scopes`.
- `feedback` MUST be the original user feedback. This is the provenance
  record: it ties the patch to a real user's real problem. Sensitive details
  (credentials, personal data, private paths) MAY be redacted, and redactions
  MUST be visibly marked (e.g. `[redacted]`); the feedback MUST NOT be
  otherwise rewritten or summarized.
- `agent` MUST disclose the backend that generated the patch. The disclosure
  MUST be truthful: a patch written by hand declares the reserved backend
  `human` rather than pretending an agent was involved. The protocol's
  machinery — opt-in, gates, provenance — applies to human-made patches all
  the same.
- `gates` MUST report the result of every gate declared in the policy.
- `human_reviewed` MUST be `true` if the policy requires human review; the
  submitting human is attesting, and false attestation is grounds for the
  maintainer to reject and ban.
- On `fallback` submissions (issues), `gates` values may be `fail` or
  `skipped`, and a `patch` field MAY carry a partial diff.

Maintainers and CI tooling can parse this block to verify conformance and to
route, label, or auto-close submissions.

## 4. Client obligations

A conforming client (such as the `auto-oss` CLI):

1. MUST NOT submit agent-generated PRs to repositories without a policy file.
2. MUST run every declared gate locally and only submit a PR when all pass;
   otherwise it MUST follow `fallback`.
3. MUST present the final diff, gate results, and submission body to the
   human for approval before submitting, when `require.human_review` is true.
4. MUST embed the metadata block, complete and truthful.
5. SHOULD respect declared `limits` without server-side enforcement.
6. MUST keep the human as the author of record: submissions are made from the
   human's account, and responsibility for the contribution stays with them.

## 5. Maintainer obligations

Publishing `auto-oss.yml` means:

1. Submissions conforming to the policy will be triaged in good faith — the
   same as any human contribution. It is not a promise to merge.
2. Non-conforming submissions MAY be closed without review.
3. Maintainers MAY change or remove the policy at any time; the policy in
   effect at submission time governs a submission.

## 6. Versioning

The `version` field and the metadata block tag (`auto-oss:v0`) carry the spec
major version. Clients encountering a higher major version than they support
MUST treat the repository as not opted in for them.

Extensions that are incompatible with this specification MUST NOT use the
name "auto-oss" or the `auto-oss.yml` / `auto-oss:vN` identifiers.

---

## License

This specification is licensed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
You are free to share, adapt, and implement it, with attribution.
