# Security policy

## Reporting a vulnerability

Please report security problems privately, not in a public issue: use **Report a vulnerability** under the
repository's **Security** tab on GitHub (private vulnerability reporting). Include what you did, what you expected,
what happened, and the version or commit.

NetLabCraft opens lab files, parses frames and console lines, and serves an MCP server, so crashes and hangs on
hostile input are in scope: a lab file, a frame, a console line or an MCP parameter that panics, loops forever or
allocates without bound is a bug we want to hear about.

## What to expect

You will get an acknowledgement, and a fix or an explanation once the report has been looked at. Please give us a
reasonable time to fix it before disclosing it publicly.

## Supported versions

There is no release yet; fixes go to `main`.

## Secrets

The repository is public and must never hold keys, tokens or personal data. `cargo xtask secrets` checks what git
would commit (`--history` checks every commit), and `.githooks/pre-commit` runs it. If you find a credential in the
history, report it the same private way.
