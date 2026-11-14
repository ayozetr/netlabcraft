# Translating NetLabCraft

> **Last reviewed:** 2026-11-14 · **Last updated:** 2026-11-14 · **Change:** major (created: catalogs move to `locales/<locale>/netlabcraft.tsv`, the languages on offer come from them) · **Target:** the storytold `craft-i18n` catalog format

NetLabCraft's interface is translated the way the storytold Crafting Apps' is
([`storytold/craft-i18n`](https://github.com/storytold/craft-i18n)): plain-text catalogs keyed by
the English text, one file per language. This page is the format, how to add a language, and the
rules. The state of each language is in [localization-parity.md](localization-parity.md).

## Where things are

| Path | What |
|---|---|
| `locales/<locale>/netlabcraft.tsv` | One language's catalog. The directory is a lowercase [BCP 47](https://www.rfc-editor.org/info/bcp47) tag: `es`, `pt-br`, `fr`, `it`, `de`, `zh-hans`. |
| `crates/ui-egui/build.rs` | Embeds every `locales/*/netlabcraft.tsv` with `include_str!`. |
| `crates/ui-egui/src/i18n.rs` | Lookup: the languages on offer, `tl`, `tr`, `trf`, `trn`, `tr_any`, `tr_event`. |
| `crates/ui-egui/src/i18n/catalog.rs` | The row format and the plural rules. |
| `crates/ui-egui/src/i18n/check.rs` | The catalog checker, a test. |

Catalogs are **always embedded**: a release is one self-contained binary (desktop, CLI and web
build alike), so there is no environment variable to point at translations as the Crafting Apps
have for `craft-i18n`. A new directory under `locales/` is a new language on the next build.

The catalogs live in this repository, in the same format as `craft-i18n`'s, so they can move
there later if NetLabCraft joins the Craft apps. NetLabCraft's own file is `netlabcraft.tsv`;
there is no shared `common.tsv`.

## The format

One string per line, three columns separated by a TAB:

```
context <TAB> English source text <TAB> translation
```

- The **source text** is the English exactly as the code writes it (`tl("Save")`). It is the key;
  there is no English catalog.
- The **context** is usually empty. A plain word (`menu`) tells apart two meanings of the same
  English (`tlc("menu", "Open")`; the plain row is the fallback). `event` marks the phrases the
  event list replaces inside the simulator's summaries (whole words only, longest first).
  Contexts starting with `@` mean something to the program, and only these exist:
  - `@id`: `language.name` holds the language's own name, which the Language menu shows. Required.
  - `@plural`: a string chosen by a count (below).
  - `@msg`: a longer message, looked up like a plain row.
- **Escapes** inside a column: `\n`, `\t`, `\\`. No others.
- **Placeholders are `{}`**, one per part, in the order the language wants them (`Opened {}`).
  NetLabCraft keeps `{}` rather than craft-i18n's `{name}`: the engine and simulator build
  messages from parts (`no route to 10.0.0.1`) and `tr_any` matches them against the rows by
  position, so named placeholders would change every call site for no gain. The checker counts
  `{}` in both columns instead of comparing names.
- A trailing `…` in the source needs one in the translation, and the other way round.
- An empty translation is an error: leave the row out and the program shows English.
- Each (context, source text) appears once per catalog.
- Lines starting with `#` and blank lines are ignored; a byte-order mark and Windows line endings
  are fine.

### Plurals

An `@plural` row's source is `one|other`, the two English forms, each with a `{}` for the count.
The translation lists the language's forms, `|`-separated, in CLDR order:

```
@plural	{} device|{} devices	{} dispositivo|{} dispositivos
```

In code, `trn("{} device", "{} devices", n)` picks the form. The number of forms is what whole
numbers 0 to 1000 reach in CLDR: `es`, `it`, `de`: 2 (1 is singular); `fr` and `pt-br`: 2 (0 and 1
are singular); `zh-hans`: 1. A language with no rule in `catalog.rs` cannot ship a `@plural` row
until its rule is added there.

### Header

Every catalog starts with the header below (the checker requires the `# Clean room:` line); copy
it from `locales/es/netlabcraft.tsv` and change the first line.

```
# <Language> (<locale>) interface catalog for NetLabCraft.
# Columns, separated by a TAB:  context <TAB> English source text <TAB> translation
#   ...
# Clean room: every entry was written from the plain meaning of the English text in ordinary
# computing vocabulary. No other product's localisation, string table or translated UI was
# consulted or copied; add entries the same way.
```

## What the program shows

For `pt-br`: the `pt-br` catalog, then the `pt` catalog if there is one, then the English text.
A missing row never errors; the English shows. A regional locale (a last subtag of two letters or
three digits) falls back to its base language; a script variant (`zh-hans`) never does.

The system locale picks the first language (the browser's on the web), and the choice in
Options ▸ Language is remembered on the desktop. `pt_BR.UTF-8` is `pt-br`; `zh_CN` and
`zh-Hans-CN` are `zh-hans`; `fr_CA` and `de_AT` are `fr` and `de`; `zh_TW` is Traditional Chinese
(`zh-hant`), which has no catalog yet, so it shows English and not Simplified. Values saved by
older versions (`en`, `es`) keep working.

The device consoles stay English on purpose (students meet those commands on real equipment), and
an activity's instructions are separate: instructors write them per language with
`activity.translate`, whatever catalogs exist.

## Add a language

1. Pick the code: lowercase BCP 47, the language and a script or region only when it matters
   (`pt-br`, `zh-hans`). Look in `locales/` first.
2. Make the starter file:

   ```sh
   cargo xtask new-locale fr "Français"
   ```

   It writes `locales/fr/netlabcraft.tsv`: the header, `@id language.name`, and every English
   string the existing catalogs have, commented out as `#<context><TAB><English><TAB>`. It never
   overwrites a catalog. Without Rust, create the file by hand: copy the header from
   `locales/es/netlabcraft.tsv`, change its first line, and add the `@id` row.
3. Translate: delete the `#` of a row and type the translation after the last TAB. Rows left
   commented or deleted show English, so a partial catalog is fine and welcome.
4. Check: `cargo test -p netlabcraft-ui-egui i18n` runs the checker and explains any row it
   rejects by file and line. `cargo test -p netlabcraft-ui-egui coverage_report -- --nocapture`
   prints each catalog's rows against the largest.

## The checker

`cargo test` checks every catalog in `locales/`: the `# Clean room:` header line; three columns;
only the three escapes; `{}` and trailing `…` parity; no empty translation; no duplicate
(context, source) per catalog; `@id language.name` present; no unknown `@` context; contexts that
are one plain word; `@plural` rows with `one|other` English and the right number of forms for the
language; a regional catalog (`es-mx`) with a row equal to its base language's (`es`) is an error,
because the program falls back anyway; locale directories named in lowercase BCP 47 and holding
only `netlabcraft.tsv`. Another test fails on a row for text no code writes any more, and the
Spanish sweep tests fail on interface text that has a row but is shown in English.

## Rules

- **Clean room.** Write every entry from the plain meaning of the English text, in ordinary
  computing vocabulary. Do not consult or copy any other product's localisation, string table or
  translated interface (Packet Tracer's or anyone else's), or its help text. Catalogs of other
  free projects are not a source for this repository's rows.
- **Not reviewed by native speakers.** Unless a language's ATTRIBUTION row says otherwise, its
  translation is a draft: written carefully, never checked by someone who speaks the language
  natively. Corrections are welcome as pull requests.
- **Spanish is the reference** for coverage: a test sweeps every window, tab, page and mode in
  Spanish and fails on text with no row.
