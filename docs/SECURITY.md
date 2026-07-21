English | [日本語](ja/SECURITY.md)

# Security

## Reporting a vulnerability

Use GitHub's private vulnerability reporting on this repository
(Security → Report a vulnerability). Please do not open public issues for
security problems.

## Threat model for `autos` users

The protocol protects maintainers from unwanted submissions. The reverse
direction needs stating: **running any of `autos`'s submission commands
(`fix`, `feat`, `docs`, `refactor`, `test`, `typo`, `resume`) against a
repository means trusting that repository**, because:

- The policy's `gates.*` commands are executed on **your** machine, inside
  the clone. A malicious opted-in repository can declare a malicious gate.
- The agent backend runs against the cloned code with edit permissions.
  Repository contents are untrusted input to the agent (prompt injection).

Mitigations today: every one of these commands shows every gate command and
requires explicit confirmation immediately before running it inside the
clone; nothing is submitted without a separate explicit confirmation.
Sandboxed gate execution is a SPEC v1 candidate. Until then, treat any of
them the same as `git clone && make` from that repository.
