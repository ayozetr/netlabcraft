# Localization parity

> **Last reviewed:** 2026-11-14 · **Last updated:** 2026-11-14 · **Change:** major (catalogs move to `locales/<locale>/netlabcraft.tsv`, the languages on offer come from them, a catalog checker) · earlier: minor (M9.11: BoardScript messages in the interface language) · earlier: minor (activity instructions per language; row counts measured again after the instructor-tool, Simulation-mode, end-device, automation and routing rounds) · **Target:** Cisco Packet Tracer 9.0

Part of [target-app-parity.md](target-app-parity.md), where it counts under Teaching.
**Localization: ~82% ready, 8–14 h** (estimated).

The reference app ships its interface in several languages through translation files. The
storytold Craft apps share their translations through
[`craft-i18n`](https://github.com/storytold/craft-i18n); NetLabCraft uses the same catalog format and lookup rules (see [translating.md](translating.md)), embedded in every build.

| Feature | Reference app | NetLabCraft | Gap and estimate |
|---|---|---|---|
| Interface language | English plus community language files | English and Spanish, and any language added as a catalog: Options ▸ Language lists every catalog by its own name, the system's language by default (the browser's on the web), remembered on the desktop | Engine errors in the status bar and the reasons a frame was dropped are translated by matching templates (`no route to {}` fits `no route to 10.0.0.1`); event summaries are translated phrase by phrase (the `event` rows of the catalog), leaving addresses, names and protocol names as they are. PDU details (layer names, fields and values) too: done |
| Catalogs | — | `locales/<locale>/netlabcraft.tsv` at the repository root, one directory per language (lowercase BCP 47: `es`, `pt-br`, `zh-hans`), in the craft-i18n row format (`context<TAB>English<TAB>translation`, `\n` for line breaks, `@id language.name`, `@plural` rows), embedded at build time so a release stays one binary; only `es` exists (1457 interface rows plus 65 `event` phrases, counted as rows with a translation). A missing row shows English; a regional locale falls back to its base language. Messages with parts use `{}` (craft-i18n uses `{name}`; kept because the engine's messages are matched against the rows by position). A checker test enforces the format on every catalog (header with the `# Clean room:` line, columns, escapes, placeholder and `…` parity, duplicates, plural forms, regional rows equal to the base) and prints each catalog's coverage; tests sweep every window, tab, page and mode in Spanish and fail on English text that has a row missing or literal text without one, check that every translated literal has a row and that every row is still used | Joining `storytold/craft-i18n` itself if NetLabCraft becomes a Craft app; more languages: 4–6 h each with review |
| Device console | English (as on real equipment) | English | Stays English on purpose: commands and output are what students meet on real devices |
| Sample labs, notes and activity texts | Activity instructions per language | Sample titles, descriptions, notes and the sample activity in Spanish (a test checks every sample has its rows); notes people write stay as written; an activity's instructions can be written in several languages (`activity.translate`, a tab per language in the Activity Wizard), and the Instructions window shows the one matching the interface language, else the main one | — |
| Right-to-left scripts, CJK fonts | — | egui default fonts only | Through `craft-fonts`, as in the other Craft apps: 3–5 h |
| BoardScript messages | — | The language's keywords are English, like a console's; its compile and run messages (38 rows) read in the interface language in the status bar, the board page's message and its output; the engine, the console and MCP keep English | None |
| Keyboard layouts and IME | OS | egui text input | Verify dead keys (Spanish `ñ`, accents) in the console: 1–2 h |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-14 | major | Catalogs move to `locales/<locale>/netlabcraft.tsv` (`es.tsv` 1457 rows and `es-events.tsv` 65 phrases become 1522 rows of one file, none lost, the phrases under the `event` context); the language list, the saved choice and the system locale mapping are data-driven (`pt_BR`, `zh_CN`, `zh-Hans-CN`, `fr_CA`, `de_AT`; `zh_TW` shows English until `zh-hant` exists); `@plural` rows and `tlc`/`trn`; a checker test over every catalog; [translating.md](translating.md). Percentage unchanged (estimated). `cargo xtask new-locale` starts a catalog. |
| 2026-11-13 | minor | BoardScript compile and run messages translated (M9.11): 38 rows, the `line {}: {}` and `stopped: {}` rows translate the message they wrap, a test compiles broken programs and runs failing ones and checks each message has a Spanish row. es.tsv counted again: 1399 rows. Percentage unchanged (estimated). |
| 2026-10-31 | minor | Localization ~80% → ~82%. es.tsv 1136 → 1347 rows (instructor windows, Simulation-mode options, tablets and smartphones, phone settings, power supplies, boards, the controller and NETCONF additions), es-events.tsv 55 → 61; activity instructions per language; BoardScript messages noted as English only. |
| 2026-10-30 | trivial | es.tsv 883 → 1136 rows (Complex PDU window, OSI view, scenarios, Programming app, PoE, SIP, IoT and cloud pages); es-events.tsv unchanged at 55. |
| 2026-10-30 | minor | Spanish sweep through every window, tab, page and mode, with tests; rows added and stale ones removed (es.tsv 566 → 850 rows). |
| 2026-10-30 | minor | Event summaries and PDU details in Spanish through a phrase catalog. |
| 2026-10-30 | minor | Engine errors and drop reasons in Spanish, matched against templates. |
| 2026-10-30 | minor | Messages with parts and the sample labs in Spanish. |
| 2026-10-30 | major | Spanish interface: catalog, language menu, system default. |
| 2026-10-30 | major | Created. |
