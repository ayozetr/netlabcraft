# Asset attribution

Every non-code asset in this repository (images, icons, fonts, example files, presets) is listed here with its author, source and licence. `cargo xtask assets` (part of `cargo xtask ci`) fails if an asset file is missing from this table.

**Policy (mandatory):** PacketCraft contains **no Cisco (or any other vendor's) iconography, device pictures, images, artwork, fonts, sample labs, activity files or help text**. Every asset is original work by PacketCraft contributors or third-party material under an open licence (OSI open source, public domain / CC0, or Creative Commons that allows redistribution). Screenshots of Packet Tracer or of any vendor's software are never committed.

Generated-in-code art is original and has no file to list: the device icons and toolbar glyphs (`crates/ui-egui/src/icons.rs`), the themes (`crates/ui-egui/src/theme.rs`) and the sample labs (`crates/engine/src/sample.rs`).

| Asset | Author | Source | Licence | Notes |
|---|---|---|---|---|
| `epaint_default_fonts` (dependency font inputs: Ubuntu-Light, Hack-Regular, emoji-icon-font, NotoEmoji-Regular) | Canonical Ltd.; Source Foundry Authors; John Slegers; The Noto Project Authors | [egui 6d518fc](https://github.com/storytold/egui/tree/6d518fc799681571963842f95b55a6b68431b15a/crates/epaint_default_fonts/fonts) | Ubuntu Font Licence 1.0; MIT + Bitstream Vera; MIT; SIL OFL 1.1 | egui's default fonts, used unmodified through the dependency; no font file is copied into this repository. |
