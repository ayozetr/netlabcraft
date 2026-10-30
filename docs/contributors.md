# Contributors

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** major (About ▸ Contributors shipped; `cargo xtask contributors`)

PacketCraft follows the storytold craftrules contributor standard
([`standards/contributors.md`](https://github.com/storytold/craftrules/blob/main/standards/contributors.md)).
This page is the local copy of the decision.

## Decision

- Two files, both compiled into the binary by `crates/ui-egui/build.rs` (nothing is read from
  disk or the network at run time; the web build has them too):
  - [`contributors/contributors.json`](../contributors/contributors.json): commit statistics per
    GitHub username, **generated** by `cargo xtask contributors` (same schema as
    `craftrules/scripts/contributors.py`); never hand-edited.
  - [`contributors/people.toml`](../contributors/people.toml): the names people chose for
    themselves. **Each contributor adds only their own entry**, in their own pull request.
- Nobody adds, edits or guesses another person's entry.

## Credit yourself

Run `python3 ../../craftrules/scripts/contributors.py --add-me .` (with `--real-name`,
`--display-name` or `--sync-github-name` if you want them), or add the block yourself in your
pull request; every field is optional:

```toml
[people.your-github-username]
real_name = "Your Name"        # shown when the viewer picks "Real name"
display_name = "Nickname"      # shown when the viewer picks "Display name"
sync_github_name = true        # or: use the name on your public GitHub profile
```

To remove your names, delete your entry in a pull request.

## In PacketCraft

- **Help ▸ About ▸ Contributors** lists everyone, like the other Craft apps: show usernames,
  display names or real names (names fall back to `@username`), sort by name, merged PRs,
  commits, lines added/deleted/delta, binary assets, first or last commit, as a grab bag of
  links or as a table. Each name links to the GitHub profile; hovering shows the full numbers.
- `cargo xtask contributors` rebuilds the JSON from `git log`:
  - A commit counts for a username only when its author email is that username's GitHub noreply
    address; any other commit counts as unattributed instead of being guessed.
  - `prs` counts `Merge pull request #N from login/…` merge commits.
  - `people_entry` is true only when every commit that added or removed `[people.login]` in
    people.toml was authored by that same username, so nobody can name someone else.
  - `github_name` stays empty (it needs the network); `sync_github_name` has no effect until a
    networked run of craftrules' script fills it.
  - `cargo xtask contributors --check` fails when the committed file is stale. It is not part of
    `cargo xtask ci` (every commit changes the numbers): refresh it before each release
    ([`releasing.md`](releasing.md)).
- There is no AI-models tab: PacketCraft's history carries no model trailers to count.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | major | Created: the craftrules decision, how to credit yourself, current state. |
| 2026-10-30 | major | About ▸ Contributors (names, sorting, grab bag/table) baked in by build.rs; `cargo xtask contributors` generates the JSON. |
