# Localization parity

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (Spanish coverage checked by tests; row counts measured again) · **Target:** Cisco Packet Tracer 9.0

Part of [target-app-parity.md](target-app-parity.md), where it counts under Teaching.
**Localization: ~80% ready, 8–14 h** (estimated).

The reference app ships its interface in several languages through translation files. The
storytold Craft apps share their translations through
[`craft-i18n`](https://github.com/storytold/craft-i18n); NetLabCraft will use the same mechanism.

| Feature | Reference app | NetLabCraft | Gap and estimate |
|---|---|---|---|
| Interface language | English plus community language files | English and Spanish: Options ▸ Language, the system's language by default (the browser's on the web), remembered on the desktop | Engine errors in the status bar and the reasons a frame was dropped are translated by matching templates (`no route to {}` fits `no route to 10.0.0.1`); event summaries are translated phrase by phrase (`i18n/es-events.tsv`), leaving addresses, names and protocol names as they are. PDU details (layer names, fields and values) too: done |
| Catalogs | — | `crates/ui-egui/i18n/es.tsv` (883 rows) and `es-events.tsv` (55 phrases; both counted as non-comment lines with a tab) in the craft-i18n row format (`English<TAB>translation`, `\n` for line breaks), embedded at build time; a missing row shows English; messages with parts use templates with `{}` (a test checks each translation keeps them). Tests sweep every window, tab, page and mode in Spanish and fail on English text that has a row missing or literal text without one, check that every translated literal has a row and that every row is still used | Move to `storytold/craft-i18n` if NetLabCraft joins the Craft apps; more languages: 4–6 h each with review |
| Device console | English (as on real equipment) | English | Stays English on purpose: commands and output are what students meet on real devices |
| Sample labs, notes and activity texts | — | Sample titles, descriptions, notes and the sample activity in Spanish (a test checks every sample has its rows); notes and activities people write stay as written | — |
| Right-to-left scripts, CJK fonts | — | egui default fonts only | Through `craft-fonts`, as in the other Craft apps: 3–5 h |
| Keyboard layouts and IME | OS | egui text input | Verify dead keys (Spanish `ñ`, accents) in the console: 1–2 h |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | Spanish sweep through every window, tab, page and mode, with tests; rows added and stale ones removed (es.tsv 566 → 850 rows). |
| 2026-10-30 | minor | Event summaries and PDU details in Spanish through a phrase catalog. |
| 2026-10-30 | minor | Engine errors and drop reasons in Spanish, matched against templates. |
| 2026-10-30 | minor | Messages with parts and the sample labs in Spanish. |
| 2026-10-30 | major | Spanish interface: catalog, language menu, system default. |
| 2026-10-30 | major | Created. |
