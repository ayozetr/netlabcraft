//! Fonts beyond egui's defaults.
//!
//! egui's built-in fonts cover Latin, Greek and Cyrillic. Chinese glyphs come from a subset of
//! Noto Sans CJK SC (assets/fonts, SIL OFL 1.1, see ATTRIBUTION.md): Basic Latin, common
//! punctuation and every GB2312 character (levels 1 and 2, about 6,700 hanzi, plus its symbols), so
//! a Simplified Chinese catalog rarely needs a glyph that is missing. It is registered as the
//! *last* fallback of the proportional and monospace families, so egui asks it only for a
//! character no other font has: Latin text renders exactly as before. The glyphs are loaded
//! lazily, as they are first drawn.

use egui::{
    FontData, FontFamily, FontPriority,
    epaint::text::{FontInsert, InsertFontFamily},
};

/// The font file: Noto Sans CJK SC Regular, subset (see ATTRIBUTION.md).
pub const CJK_SUBSET: &[u8] = include_bytes!("../../../assets/fonts/NotoSansCJKsc-Regular-subset.otf");

const NAME: &str = "noto-sans-cjk-sc-subset";

/// Registers the Chinese fallback font. Call once at start-up, before the first frame, in the
/// desktop app and in the web build.
pub fn install(ctx: &egui::Context) {
    let last = |family| InsertFontFamily { family, priority: FontPriority::Lowest };
    ctx.add_font(FontInsert::new(NAME, FontData::from_static(CJK_SUBSET), vec![last(FontFamily::Proportional), last(FontFamily::Monospace)]));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a frame, so the context has its fonts.
    fn frame(ctx: &egui::Context) {
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
    }

    fn has(ctx: &egui::Context, family: FontFamily, text: &str) -> bool {
        frame(ctx);
        ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::new(14.0, family), text))
    }

    #[test]
    fn the_subset_has_the_glyphs_of_a_chinese_interface() {
        let sample = "网络实验室：设备、交换机、路由器；保存并打开文件（确定）…";
        let plain = egui::Context::default();
        assert!(!has(&plain, FontFamily::Proportional, sample), "the default fonts have no Chinese");
        let ctx = egui::Context::default();
        install(&ctx);
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            assert!(has(&ctx, family.clone(), sample), "{family:?}");
        }
        // Level 2 hanzi (rarely used) and the first and last hanzi of GB2312 (levels 1 and 2).
        assert!(has(&ctx, FontFamily::Proportional, "啊座齄"));
    }

    #[test]
    fn latin_text_still_comes_from_the_default_fonts() {
        let (plain, ctx) = (egui::Context::default(), egui::Context::default());
        install(&ctx);
        let width = |c: &egui::Context| {
            frame(c);
            c.fonts_mut(|f| {
                f.layout_no_wrap("Switch Gi0/1 – ñ é 192.168.0.1".into(), egui::FontId::proportional(14.0), egui::Color32::WHITE).size().x
            })
        };
        assert_eq!(width(&plain), width(&ctx));
    }
}
