//! The Palette window: the outline and fill of drawings. It shows while a drawing tool is active
//! (what it sets is the default for the shapes drawn next) or while a drawing is selected (what it
//! sets is applied to that drawing through `note.style`).

use netlabcraft_model::{Annotation, MAX_OUTLINE_WIDTH, Shape, parse_color};
use serde_json::{Value, json};

use crate::i18n::tl;
use crate::{PacketApp, Tool};

/// Fill colour of a drawing that has none, as the canvas has always drawn it.
const DEFAULT_FILL: [u8; 3] = [0xbc, 0xd9, 0xf2];

/// What the Palette edits.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub outline: bool,
    pub outline_color: [u8; 3],
    pub outline_width: u8,
    pub fill: bool,
    pub fill_color: [u8; 3],
}

fn shaded(c: [u8; 3], f: f32) -> [u8; 3] {
    let m = |v: u8| (f32::from(v) * f).clamp(0.0, 255.0) as u8;
    [m(c[0]), m(c[1]), m(c[2])]
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

impl Default for Style {
    fn default() -> Style {
        Style { outline: true, outline_color: shaded(DEFAULT_FILL, 0.6), outline_width: 2, fill: true, fill_color: DEFAULT_FILL }
    }
}

impl Style {
    /// How an existing drawing looks now (what the canvas draws when a setting was never made).
    pub fn of(a: &Annotation) -> Style {
        let fill_color = a.color.as_deref().and_then(parse_color).unwrap_or(DEFAULT_FILL);
        let line = matches!(a.shape, Shape::Line | Shape::Freehand);
        let outline_color = a.outline_color.as_deref().and_then(parse_color).unwrap_or_else(|| shaded(fill_color, if line { 0.6 } else { 0.7 }));
        Style { outline: a.has_outline(), outline_color, outline_width: a.outline_px(), fill: !a.no_fill, fill_color }
    }

    pub fn fill_hex(&self) -> String {
        hex(self.fill_color)
    }

    pub fn outline_hex(&self) -> String {
        hex(self.outline_color)
    }

    /// The `note.add` parameters for a new drawing of this shape.
    pub fn params(&self, shape: Shape) -> Value {
        if matches!(shape, Shape::Line | Shape::Freehand) {
            return json!({"outline_color": hex(self.outline_color), "outline_width": self.outline_width});
        }
        json!({
            "outline": self.outline,
            "outline_color": hex(self.outline_color),
            "outline_width": self.outline_width,
            "fill": if self.fill { hex(self.fill_color) } else { "none".to_string() },
        })
    }

    /// The `note.style` parameters that turn `from` into `self` (empty when nothing differs).
    fn diff(&self, from: &Style, shape: Shape) -> Option<Value> {
        let line = matches!(shape, Shape::Line | Shape::Freehand);
        let mut p = serde_json::Map::new();
        if !line && self.outline != from.outline {
            p.insert("outline".into(), json!(self.outline));
        }
        if self.outline_color != from.outline_color {
            p.insert("outline_color".into(), json!(hex(self.outline_color)));
        }
        if self.outline_width != from.outline_width {
            p.insert("outline_width".into(), json!(self.outline_width));
        }
        if !line && (self.fill != from.fill || (self.fill && self.fill_color != from.fill_color)) {
            p.insert("fill".into(), json!(if self.fill { hex(self.fill_color) } else { "none".to_string() }));
        }
        (!p.is_empty()).then_some(Value::Object(p))
    }
}

/// The Palette's state besides the shown window.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// The default for new drawings, for the session.
    pub style: Style,
    /// The edited copy of the selected drawing while a change is waiting for the pointer to be released.
    edit: Option<Style>,
}

/// What the Palette is editing now.
enum Target {
    New(Shape),
    Selected(u32, Shape),
}

fn target(app: &PacketApp) -> Option<Target> {
    match app.ui.tool {
        Tool::Draw(s) if s != Shape::Text => Some(Target::New(s)),
        Tool::Select => {
            let n = app.ui.selected_note?;
            let a = app.session.topology.annotations.iter().find(|a| a.id == n)?;
            (a.shape != Shape::Text).then_some(Target::Selected(n, a.shape))
        }
        _ => None,
    }
}

fn colour_button(ui: &mut egui::Ui, c: &mut [u8; 3], name: &'static str) -> bool {
    let r = ui.color_edit_button_srgb(c);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tl(name)));
    r.changed()
}

pub fn window(app: &mut PacketApp, ctx: &egui::Context) {
    if app.session.physical {
        return;
    }
    let Some(target) = target(app) else {
        app.ui.palette.edit = None;
        return;
    };
    let (shape, selected) = match target {
        Target::New(s) => (s, None),
        Target::Selected(n, s) => (s, Some(n)),
    };
    let current = selected.and_then(|n| app.session.topology.annotations.iter().find(|a| a.id == n)).map(Style::of);
    let mut st = match (&current, &app.ui.palette.edit) {
        (Some(_), Some(e)) => e.clone(),
        (Some(c), None) => c.clone(),
        (None, _) => app.ui.palette.style.clone(),
    };
    let line = matches!(shape, Shape::Line | Shape::Freehand);
    // Starts in the workspace's bottom right corner; it can be dragged anywhere.
    let corner = app.ui.canvas_rect.map_or(egui::pos2(900.0, 500.0), |r| r.right_bottom() - egui::vec2(12.0, 12.0));
    let mut changed = false;
    let mut switch = None;
    egui::Window::new(tl("Palette")).id(egui::Id::unique("palette")).pivot(egui::Align2::RIGHT_BOTTOM).default_pos(corner).resizable(false).show(
        ctx,
        |ui| {
            if selected.is_none() {
                ui.horizontal(|ui| {
                    for (s, name) in [
                        (Shape::Line, "Line"),
                        (Shape::Rectangle, "Rectangle"),
                        (Shape::Ellipse, "Ellipse"),
                        (Shape::Freehand, "Freeform"),
                        (Shape::Polygon, "Polygon"),
                    ] {
                        if ui.selectable_label(shape == s, tl(name)).clicked() {
                            switch = Some(s);
                        }
                    }
                });
                ui.separator();
            }
            ui.horizontal(|ui| {
                if line {
                    ui.label(tl("Outline"));
                } else {
                    changed |= ui.checkbox(&mut st.outline, tl("Outline")).changed();
                }
                changed |= colour_button(ui, &mut st.outline_color, "Outline colour");
            });
            ui.horizontal(|ui| {
                ui.label(tl("Width"));
                let mut w = i32::from(st.outline_width);
                if ui.add(egui::DragValue::new(&mut w).range(1..=i32::from(MAX_OUTLINE_WIDTH))).changed() {
                    st.outline_width = u8::try_from(w).unwrap_or(1);
                    changed = true;
                }
            });
            if !line {
                ui.separator();
                ui.horizontal(|ui| {
                    changed |= ui.radio_value(&mut st.fill, false, tl("No fill")).changed();
                    changed |= ui.radio_value(&mut st.fill, true, tl("Fill")).changed();
                    changed |= colour_button(ui, &mut st.fill_color, "Fill colour");
                });
            }
        },
    );
    if let Some(s) = switch {
        app.set_tool(Tool::Draw(s));
    }
    match (selected, current) {
        (Some(n), Some(cur)) => {
            if changed {
                app.ui.palette.edit = Some(st.clone());
            }
            // A colour dragged in the picker is applied when the pointer is let go: one undo step.
            if app.ui.palette.edit.is_some() && !ctx.input(|i| i.pointer.any_down()) {
                app.ui.palette.edit = None;
                if let Some(p) = st.diff(&cur, shape).map(|mut p| {
                    p["id"] = json!(n);
                    p
                }) {
                    app.run("note.style", p);
                }
            }
        }
        _ => {
            if changed {
                app.ui.palette.style = st;
            }
        }
    }
}
