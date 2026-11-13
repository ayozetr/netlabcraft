# Security

NetLabCraft opens lab files, decodes frames, parses console lines and serves an MCP server, all of it
untrusted input, so we take reports seriously.

## Reporting a vulnerability

Please report it privately: on GitHub, open the repository's **Security** tab and choose
**Report a vulnerability**. Don't open a public issue for it. Say what's affected, how to reproduce
it, and what an attacker gains. We'll reply within a week and tell you what we'll do.

## What counts

- A crash, hang or runaway memory use on any lab file, frame, console line or MCP parameter
  (never-crash is a hard rule: [AGENTS.md](AGENTS.md)).
- An MCP tool or command that does more than its documented effect, or touches files outside the ones
  it was given.
- A key, token or other secret reaching a log, a lab file, a capture or the repository
  (`cargo xtask secrets` checks what git would commit, `--history` every commit).

## Supported versions

Fixes go to `main` and into the next release; the latest release is the supported one. Older releases are not patched.
