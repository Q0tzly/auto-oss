# Security

## Reporting a vulnerability

Use GitHub's private vulnerability reporting on this repository
(Security → Report a vulnerability). Please do not open public issues for
security problems.

## Threat model for `autos` users

The protocol protects maintainers from unwanted submissions. The reverse
direction needs stating: **running `autos fix` against a repository means
trusting that repository**, because:

- The policy's `gates.*` commands are executed on **your** machine, inside
  the clone. A malicious opted-in repository can declare a malicious gate.
- The agent backend runs against the cloned code with edit permissions.
  Repository contents are untrusted input to the agent (prompt injection).

Mitigations today: `autos fix` shows every gate command and requires explicit
confirmation before cloning or invoking the backend; gates run inside the
clone's working directory; nothing is submitted without a second explicit
confirmation. Sandboxed gate execution is a SPEC v1 candidate. Until then,
treat `autos fix <repo>` with the same caution as `git clone && make` from
that repository.
