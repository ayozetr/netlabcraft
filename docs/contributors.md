# Contributors

> **Last reviewed:** 2026-11-17 · **Last updated:** 2026-11-17 · **Change:** minor (consistency pass: the Contributors workflow is named in the refresh step) · earlier: minor (no reference to a repository that is not public) · earlier: minor (co-authors credited)

NetLabCraft credits contributors the way the other Crafting Apps do: by GitHub username, with names
that people choose for themselves. This page is the decision and how to use it.

## Decision

- Two files, both compiled into the binary by `crates/ui-egui/build.rs` (nothing is read from
  disk or the network at run time; the web build has them too):
  - [`contributors/contributors.json`](../contributors/contributors.json): commit statistics per
    GitHub username, **generated** by `cargo xtask contributors` (a schema of its own,
    version 1); never hand-edited.
  - [`contributors/people.toml`](../contributors/people.toml): the names people chose for
    themselves. **Each contributor adds only their own entry**, in their own pull request.
- Nobody adds, edits or guesses another person's entry.

## Credit yourself

Add this block yourself in your own pull request (commit it with your own account); every field
is optional:

```toml
[people.your-github-username]
real_name = "Your Name"        # shown when the viewer picks "Real name"
display_name = "Nickname"      # shown when the viewer picks "Display name"
sync_github_name = true        # or: use the name on your public GitHub profile
```

To remove your names, delete your entry in a pull request.

## In NetLabCraft

- **Help ▸ About ▸ Contributors** lists everyone, like the other Craft apps: show usernames,
  display names or real names (names fall back to `@username`), sort by name, merged PRs,
  commits, lines added/deleted/delta, binary assets, first or last commit, as a grab bag of
  links or as a table. Each name links to the GitHub profile; hovering shows the full numbers.
- `cargo xtask contributors` rebuilds the JSON from `git log`:
  - A commit counts for a username only when its author email is that username's GitHub noreply
    address; any other commit counts as unattributed instead of being guessed.
  - Co-authors named in `Co-authored-by: Name <noreply email>` trailers are credited with the
    commit too: the commit, its dates and its lines (a shared commit can't be split).
  - Usernames keep their own case, as GitHub shows them.
  - `prs` counts `Merge pull request #N from login/…` merge commits.
  - `people_entry` is true only when every commit that added or removed `[people.login]` in
    people.toml was authored by that same username, so nobody can name someone else.
  - `github_name` stays empty (it needs the network); `sync_github_name` has no effect until a
    a networked tool fills it (nothing does yet).
  - `cargo xtask contributors --check` fails when the committed file is stale. It is not part of
    `cargo xtask ci` (every commit changes the numbers): refresh it before each release
    ([`releasing.md`](releasing.md)): the `Contributors` workflow (Actions, run by hand) runs it in CI and uploads the file as an artifact.
- There is no AI-models tab: NetLabCraft's history carries no model trailers to count.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-17 | minor | Consistency pass: the refresh step names the `Contributors` workflow, as releasing.md does. |
| 2026-11-17 | minor | No more links to a `craftrules` repository (it is not public): the page describes the standard itself and the add-yourself block. |
| 2026-10-31 | minor | Co-authors from `Co-authored-by` trailers are credited in About ▸ Contributors. |
| 2026-10-30 | major | Created: the craftrules decision, how to credit yourself, current state. |
| 2026-10-30 | major | About ▸ Contributors (names, sorting, grab bag/table) baked in by build.rs; `cargo xtask contributors` generates the JSON. |
