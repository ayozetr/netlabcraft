# Contributors

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** major (created)

PacketCraft follows the storytold craftrules contributor standard
([`standards/contributors.md`](https://github.com/storytold/craftrules/blob/main/standards/contributors.md)).
This page is the local copy of the decision.

## Decision

- Two files, both compiled into the binary once the About window lists contributors (nothing is
  read from disk or the network at run time; the web build has them too):
  - `contributors/contributors.json`: commit statistics per GitHub username, **generated** by
    `craftrules/scripts/contributors.py`; never hand-edited. Not created yet: it comes with the
    first contribution from outside.
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

- Not shown in the app yet: an About ▸ Contributors view like the other Craft apps' is planned
  with the first release ([`releasing.md`](releasing.md)).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | major | Created: the craftrules decision, how to credit yourself, current state. |
