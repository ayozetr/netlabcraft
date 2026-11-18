//! The buttons at the right of the physical bar: the `+` menu that creates what can be made at
//! the level shown (a city on the Intercity map, a building or a container in a city, a closet or
//! a container in a building, furniture in a closet or a container), and the Home button that
//! goes to the working closet.
//!
//! Both are drawn in code. The menu is its own popup so the keyboard works the same everywhere:
//! Enter on the button opens it, the arrows move, Enter chooses, Esc closes. What a level cannot
//! make is not offered; the same creations by shortcut say why they did nothing.

use egui::{Align2, Color32, Key, Modifiers, Sense, Stroke, StrokeKind, WidgetInfo, pos2, vec2};
use netlabcraft_model::furniture::FurnitureKind;
use netlabcraft_model::physical::PlaceKind;
use serde_json::json;

use crate::i18n::{tl, trf};
use crate::prefs_look::HintExt;
use crate::shortcuts::{self, Act};
use crate::{PacketApp, theme};

/// Something the `+` menu can create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Create {
    City,
    Building,
    Closet,
    Container,
    Furniture(FurnitureKind),
}

impl Create {
    /// Every item, in the order the menu lists them.
    pub const ALL: [Create; 10] = [
        Create::City,
        Create::Building,
        Create::Closet,
        Create::Container,
        Create::Furniture(FurnitureKind::Rack),
        Create::Furniture(FurnitureKind::Table),
        Create::Furniture(FurnitureKind::Shelf),
        Create::Furniture(FurnitureKind::InventoryShelf),
        Create::Furniture(FurnitureKind::CablePegboard),
        Create::Furniture(FurnitureKind::ControlPanel),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Create::City => "Create New City",
            Create::Building => "Create New Building",
            Create::Closet => "Create New Closet",
            Create::Container => "Create New Generic Container",
            Create::Furniture(FurnitureKind::Rack) => "Create New Rack",
            Create::Furniture(FurnitureKind::Table) => "Create New Table",
            Create::Furniture(FurnitureKind::Shelf) => "Create New Shelf",
            Create::Furniture(FurnitureKind::InventoryShelf) => "Create New Inventory Shelf",
            Create::Furniture(FurnitureKind::CablePegboard) => "Create New Cable Pegboard",
            Create::Furniture(FurnitureKind::ControlPanel) => "Create New Industrial Control Panel (ICP)",
        }
    }

    /// The shortcut that makes it (the furniture has none).
    pub fn act(self) -> Option<Act> {
        match self {
            Create::City => Some(Act::NewCity),
            Create::Building => Some(Act::NewBuilding),
            Create::Closet => Some(Act::NewCloset),
            Create::Container => Some(Act::NewContainer),
            Create::Furniture(_) => None,
        }
    }

    fn place_kind(self) -> Option<PlaceKind> {
        match self {
            Create::City => Some(PlaceKind::City),
            Create::Building => Some(PlaceKind::Building),
            Create::Closet => Some(PlaceKind::Closet),
            Create::Container => Some(PlaceKind::Container),
            Create::Furniture(_) => None,
        }
    }
}

/// Where in the physical view the user is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    /// The logical workspace.
    Logical,
    Intercity,
    City,
    Building,
    /// A wiring closet or a container.
    Holder,
}

fn level(app: &PacketApp) -> Level {
    if !app.session.physical {
        return Level::Logical;
    }
    match app.session.place_view.and_then(|p| app.session.topology.place(p)).map(|p| p.kind) {
        None => Level::Intercity,
        Some(PlaceKind::City) => Level::City,
        Some(PlaceKind::Building) => Level::Building,
        Some(PlaceKind::Closet | PlaceKind::Container) => Level::Holder,
    }
}

/// Why `what` cannot be created at the level shown (none: it can).
pub fn reason(app: &PacketApp, what: Create) -> Option<&'static str> {
    let at = level(app);
    if at == Level::Logical {
        return Some("Switch to the Physical view first.");
    }
    let ok = match what {
        Create::City => at == Level::Intercity,
        Create::Building => at == Level::City,
        Create::Closet => at == Level::Building,
        Create::Container => matches!(at, Level::City | Level::Building),
        Create::Furniture(_) => at == Level::Holder,
    };
    if ok {
        return None;
    }
    Some(match what {
        Create::City => "Go to the Intercity map to create a city.",
        Create::Building => "Open a city first.",
        Create::Closet => "Open a building first.",
        Create::Container => "Open a city or a building first.",
        Create::Furniture(_) => "Open a wiring closet or a container first.",
    })
}

/// What the `+` menu offers at the level shown.
pub fn items(app: &PacketApp) -> Vec<Create> {
    Create::ALL.into_iter().filter(|c| reason(app, *c).is_none()).collect()
}

/// Creates `what` here, or says in the status line why it cannot be.
pub fn create(app: &mut PacketApp, what: Create) {
    if let Some(why) = reason(app, what) {
        let msg = format!("{}: {}", tl(what.label()), tl(why));
        app.ui.status = Some((msg, true));
        return;
    }
    if let Some(kind) = what.place_kind() {
        crate::physical::add_kind(app, kind, None);
    } else if let Create::Furniture(kind) = what
        && let Some(place) = crate::closet::current(app)
        && let Some(v) = app.run("furniture.add", json!({"kind": kind.id(), "place": place}))
    {
        let name = v.get("name").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
        app.info(trf("Added {}.", &[&name]));
    }
}

/// Shift+H and the Home button: into the working closet from anywhere.
pub fn go_home(app: &mut PacketApp) {
    match app.session.topology.working_closet() {
        None => app.ui.status = Some((tl(NO_CLOSET).to_string(), true)),
        Some(id) => {
            if !app.session.physical {
                app.set_physical(true);
            }
            crate::physical::enter(app, id);
        }
    }
}

const NO_CLOSET: &str = "No wiring closet yet: create one with +";

/// The state of the `+` menu.
#[derive(Debug, Clone, Default)]
pub struct MenuState {
    pub open: bool,
    pub sel: usize,
}

/// Closes the menu (Esc); whether it was open.
pub fn close_menu(app: &mut PacketApp) -> bool {
    std::mem::replace(&mut app.ui.physical.menu.open, false)
}

/// The `+` button and, while it is open, its menu.
pub fn create_button(app: &mut PacketApp, ui: &mut egui::Ui) {
    let tk = theme::tokens(app.ui.dark);
    let items = items(app);
    let mut chosen = None;
    // Keys first, so the Enter that chooses is not also read by the button that is focused.
    if app.ui.physical.menu.open {
        let n = items.len().max(1);
        let (down, up, enter, esc) = ui.ctx().input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        let m = &mut app.ui.physical.menu;
        if down {
            m.sel = (m.sel + 1) % n;
        }
        if up {
            m.sel = (m.sel + n - 1) % n;
        }
        if enter {
            chosen = items.get(m.sel).copied();
            m.open = false;
        }
        if esc {
            m.open = false;
        }
    }
    let (r, resp) = ui.allocate_exact_size(vec2(32.0, 28.0), Sense::click());
    let label = tl("Create New…");
    let resp = resp.hint(label);
    resp.widget_info(|| WidgetInfo::labeled(egui::accesskit::Role::Button, true, label));
    let open = app.ui.physical.menu.open;
    let p = ui.painter_at(r);
    if open {
        p.rect_filled(r.shrink(1.5), 7.0, tk.pill);
    } else if resp.hovered() {
        p.rect_filled(r.shrink(1.5), 7.0, tk.pill.gamma_multiply(0.5));
    }
    p.rect_stroke(r, 8.0, Stroke::new(1.5, tk.bar_text), StrokeKind::Inside);
    if resp.has_focus() {
        p.rect_stroke(r.expand(2.0), 9.0, Stroke::new(2.0, tk.selection), StrokeKind::Outside);
    }
    let c = r.center();
    let plus = Stroke::new(2.4, tk.pill_text);
    p.line_segment([c - vec2(7.0, 0.0), c + vec2(7.0, 0.0)], plus);
    p.line_segment([c - vec2(0.0, 7.0), c + vec2(0.0, 7.0)], plus);
    if resp.clicked() {
        let m = &mut app.ui.physical.menu;
        m.open = !m.open;
        m.sel = 0;
    }
    if app.ui.physical.menu.open {
        let mut sel = app.ui.physical.menu.sel;
        let area = egui::Area::new(egui::Id::unique("create_menu"))
            .order(egui::Order::Foreground)
            .pivot(Align2::RIGHT_TOP)
            .fixed_pos(r.right_bottom() + vec2(0.0, 3.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(280.0);
                    for (i, what) in items.iter().enumerate() {
                        let mut b = egui::Button::new(tl(what.label())).selected(i == sel).min_size(vec2(268.0, 24.0));
                        if let Some(act) = what.act() {
                            b = b.shortcut_text(shortcuts::text(act));
                        }
                        let rb = ui.add(b);
                        if rb.hovered() {
                            sel = i;
                        }
                        if rb.clicked() {
                            chosen = Some(*what);
                        }
                    }
                });
            });
        app.ui.physical.menu.sel = sel;
        let outside = ui
            .ctx()
            .input(|i| i.pointer.primary_pressed() && i.pointer.interact_pos().is_some_and(|at| !area.response.rect.contains(at) && !r.contains(at)));
        if outside || chosen.is_some() {
            app.ui.physical.menu.open = false;
        }
    }
    if let Some(what) = chosen {
        create(app, what);
    }
}

/// The Home button, last at the right of the bar: a house on blue, to the working closet.
pub fn home_button(app: &mut PacketApp, ui: &mut egui::Ui) {
    let tk = theme::tokens(app.ui.dark);
    let enabled = app.session.topology.working_closet().is_some();
    let (r, resp) = ui.allocate_exact_size(vec2(32.0, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    let name = tl("Go to Working Closet");
    let tip = if enabled { trf("Go to Working Closet ({})", &[&shortcuts::text(Act::WorkingCloset)]) } else { tl(NO_CLOSET).to_string() };
    let resp = resp.hint(tip);
    resp.widget_info(|| WidgetInfo::labeled(egui::accesskit::Role::Button, enabled, name));
    let p = ui.painter_at(r);
    let fill = if !enabled {
        tk.pill.gamma_multiply(0.35)
    } else if resp.hovered() {
        tk.pill.gamma_multiply(0.8)
    } else {
        tk.pill
    };
    p.rect_filled(r, 8.0, fill);
    if resp.has_focus() {
        p.rect_stroke(r.expand(2.0), 9.0, Stroke::new(2.0, tk.selection), StrokeKind::Outside);
    }
    house(&p, r.center(), if enabled { Color32::WHITE } else { Color32::WHITE.gamma_multiply(0.55) }, fill);
    if enabled && resp.clicked() {
        go_home(app);
    }
}

/// A house: a roof, walls, a chimney and a door cut out in `cut`.
fn house(p: &egui::Painter, c: egui::Pos2, white: Color32, cut: Color32) {
    p.add(egui::Shape::convex_polygon(vec![pos2(c.x - 10.5, c.y - 0.5), pos2(c.x, c.y - 9.5), pos2(c.x + 10.5, c.y - 0.5)], white, Stroke::NONE));
    p.rect_filled(egui::Rect::from_min_max(pos2(c.x - 7.0, c.y - 1.0), pos2(c.x + 7.0, c.y + 8.0)), 0.5, white);
    p.rect_filled(egui::Rect::from_min_max(pos2(c.x + 4.5, c.y - 8.5), pos2(c.x + 7.5, c.y - 3.0)), 0.0, white);
    p.rect_filled(egui::Rect::from_min_max(pos2(c.x - 2.0, c.y + 2.5), pos2(c.x + 2.0, c.y + 8.0)), 0.0, cut);
}

impl PacketApp {
    /// Opens the `+` menu (the contact sheets draw it open).
    pub fn open_create_menu(&mut self) {
        self.ui.physical.menu.open = true;
        self.ui.physical.menu.sel = 0;
    }
}
