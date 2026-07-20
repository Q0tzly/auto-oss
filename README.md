# auto-oss

A contribution protocol for **user-side coding agents**: when a user of an
open-source project hits a problem, their own agent turns that feedback into a
patch and submits it upstream — under conditions the repository has declared
in advance by publishing an `auto-oss.yml` policy file.

Existing automation (OpenHands Resolver, Copilot coding agent, Sentry Seer)
works for the repository's *owners*. auto-oss is the other direction: the
*outside user* is the starting point, and the protocol exists so that
agent-made contributions arrive in a form maintainers can trust — opt-in only,
quality-gated, disclosed, and always with a human as the author of record.

- [SPEC.md](SPEC.md) — protocol specification (v0 draft): the `auto-oss.yml`
  policy file and the submission metadata block
- [DESIGN.md](DESIGN.md) — design notes and rationale (Japanese)

Status: design stage. The reference CLI is not implemented yet.
