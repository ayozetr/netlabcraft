# Contributing

Thanks for helping. The rules every change follows are in [AGENTS.md](AGENTS.md); the
mistakes they come from are in [docs/pitfalls.md](docs/pitfalls.md). In short:

- **Pick work from [docs/gaps.md](docs/gaps.md)** (ranked; pick from the top). Say what you are taking by opening a
  draft pull request early, so nobody does the same work.
- **One task per branch and pull request**, on a branch named `feature/`, `bugfix/`, `hotfix/`, `refactor/`,
  `docs/`, `test/` or `chore/` plus the task id (`feature/m6.41-ospf-timers`). Never commit to `main`. One task id
  per commit (`M6.41: OSPF hello and dead timers`).
- **Where things go:** [docs/extending.md](docs/extending.md).
- **Clean room.** Packet Tracer and other vendors' software are only observed black-box. No vendor icons,
  artwork, help text or sample labs, and no GPL/LGPL/AGPL code (not even reading it). See AGENTS.md.
- **Never crash, safe Rust only, other programs through `netlabcraft_core::process`, mutexes from
  `netlabcraft_core::sync`, no secrets or personal data anywhere.**

Before you ask for review:

```sh
cargo xtask ci        # fmt, clippy -D warnings, tests, doctests, assets, layers, secrets, wasm, parity
```

Enable the secret-scan hook once per clone: `git config core.hooksPath .githooks`.

Code is dual-licensed under MIT or Apache-2.0 ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)); by
contributing you agree your work is licensed the same way.
