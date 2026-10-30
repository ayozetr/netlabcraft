# Localization parity

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** major (Spanish interface) · **Target:** Cisco Packet Tracer 9.0

Part of [target-app-parity.md](target-app-parity.md). **Localization: ~45% ready, 15–25 h**
(estimated).

The reference app ships its interface in several languages through translation files. The
storytold Craft apps share their translations through
[`craft-i18n`](https://github.com/storytold/craft-i18n); PacketCraft will use the same mechanism.

| Feature | Reference app | PacketCraft | Gap and estimate |
|---|---|---|---|
| Interface language | English plus community language files | English and Spanish: Options ▸ Language, the system's language by default (the browser's on the web), remembered on the desktop | Messages built from parts (status lines, some table texts) are still English: 4–6 h |
| Catalogs | — | `crates/ui-egui/i18n/es.tsv` (237 rows) in the craft-i18n row format (`English<TAB>translation`), embedded at build time; a missing row shows English | Move to `storytold/craft-i18n` if PacketCraft joins the Craft apps; more languages: 4–6 h each with review |
| Device console | English (as on real equipment) | English | Stays English on purpose: commands and output are what students meet on real devices |
| Sample labs, notes and activity texts | — | English | Translate with the interface: 2–4 h |
| Right-to-left scripts, CJK fonts | — | egui default fonts only | Through `craft-fonts`, as in the other Craft apps: 3–5 h |
| Keyboard layouts and IME | OS | egui text input | Verify dead keys (Spanish `ñ`, accents) in the console: 1–2 h |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | major | Spanish interface: catalog, language menu, system default. |
| 2026-10-30 | major | Created. |
