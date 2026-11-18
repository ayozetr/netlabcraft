//! Furniture in a wiring closet: racks, shelves and tables that devices stand on, and the two
//! passive devices that live there, the patch panel and the power distribution device.
//!
//! * A **rack** has numbered units (U); an item takes one or more consecutive units and no two
//!   items overlap. A **shelf** has levels and holds a few items loose on each. A **table** holds
//!   a few items loose.
//! * A **patch panel** has ports. A patch wire joins a panel port to another panel port (a patch
//!   cord) or to a free device port (a drop); a panel port takes one drop and one cord. A device
//!   port at each end of a patch cord, each through its own drop, is a *patch path*: the two devices are cabled through the panels, so
//!   the path makes an ordinary cable between them (the devices behave as if cabled directly) and
//!   removing a wire of the path unplugs it again.
//! * A **backbone** is a cord between two panel or wall mount ports that stand in two different
//!   places (closet to closet, building to building). It is stored as a patch cord whose ends
//!   are in different places, so a drop, a backbone and a drop make a patch path like any other:
//!   the two devices, each in its own closet, are cabled through it.
//! * A **power distribution device** has outlets. A device plugged into one follows the strip's
//!   power switch: switching the strip off switches the device off, and the device cannot be
//!   switched on while the strip is off.
//!
//! Everything is stored in [`Physical`](crate::physical::Physical) and absent from files that
//! never used it.

use std::collections::BTreeSet;

use netlabcraft_core::DeviceId;
use serde::{Deserialize, Serialize};

use crate::physical::{PlaceKind, clean_name};
use crate::{End, Link, ModelError, Result, Topology, finite};

/// Most pieces of furniture in a lab.
pub const MAX_FURNITURE: usize = 200;
/// Most passive devices (patch panels and power distribution devices) in a lab.
pub const MAX_FITTINGS: usize = 400;
/// Most patch wires in a lab.
pub const MAX_PATCH_WIRES: usize = 4_000;
/// Most rack units one item takes.
pub const MAX_ITEM_UNITS: u32 = 4;
/// Items a shelf holds on each level.
pub const SHELF_CAPACITY: usize = 4;
/// Items a table holds.
pub const TABLE_CAPACITY: usize = 8;
/// A table's width in cells as made, and its depth in rows of cells.
pub const TABLE_COLS: u32 = 4;
pub const TABLE_ROWS: u32 = 2;
/// The widths (cells in a row) and depths (rows) a table may be set to.
pub const TABLE_WIDTH_RANGE: (u32, u32) = (2, 8);
pub const TABLE_DEPTH_RANGE: (u32, u32) = (1, 6);
/// Hooks in a row of a cable pegboard.
pub const PEGS_PER_ROW: u32 = 6;
/// Most cables hung on pegboards in a lab.
pub const MAX_HANGS: usize = 2_000;

/// Whether an industrial control panel's DIN rail takes a device of this catalogue model: PLCs,
/// HMI panels, data historians, industrial switches and routers, signal generators and
/// batteries (power supplies).
pub fn icp_accepts(model: &str) -> bool {
    matches!(model, "PLC" | "HMI-Panel" | "Data-Historian" | "Switch-IND-8" | "Router-IND" | "IoT-Signal" | "IoT-Battery")
}

/// What a piece of furniture is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FurnitureKind {
    Rack,
    Shelf,
    Table,
    /// A shelf of labelled storage bins where devices wait in storage: they are switched off
    /// when put there, and shown dimmed.
    #[serde(rename = "inventory-shelf")]
    InventoryShelf,
    /// A board with rows of hooks to hang cables on; no device stands on it.
    #[serde(rename = "pegboard")]
    CablePegboard,
    /// An industrial control panel: a wall panel with a DIN rail that industrial devices mount on.
    #[serde(rename = "icp")]
    ControlPanel,
}

impl FurnitureKind {
    pub const ALL: [FurnitureKind; 6] = [
        FurnitureKind::Rack,
        FurnitureKind::Shelf,
        FurnitureKind::Table,
        FurnitureKind::InventoryShelf,
        FurnitureKind::CablePegboard,
        FurnitureKind::ControlPanel,
    ];

    pub fn id(self) -> &'static str {
        match self {
            FurnitureKind::Rack => "rack",
            FurnitureKind::Shelf => "shelf",
            FurnitureKind::Table => "table",
            FurnitureKind::InventoryShelf => "inventory-shelf",
            FurnitureKind::CablePegboard => "pegboard",
            FurnitureKind::ControlPanel => "icp",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FurnitureKind::Rack => "Rack",
            FurnitureKind::Shelf => "Shelf",
            FurnitureKind::Table => "Table",
            FurnitureKind::InventoryShelf => "Inventory Shelf",
            FurnitureKind::CablePegboard => "Cable Pegboard",
            FurnitureKind::ControlPanel => "Industrial Control Panel",
        }
    }

    pub fn from_id(s: &str) -> Option<FurnitureKind> {
        match s.trim().to_ascii_lowercase().replace(['_', ' '], "-").as_str() {
            "rack" => Some(FurnitureKind::Rack),
            "shelf" => Some(FurnitureKind::Shelf),
            "table" => Some(FurnitureKind::Table),
            "inventory-shelf" | "inventory" => Some(FurnitureKind::InventoryShelf),
            "pegboard" | "cable-pegboard" => Some(FurnitureKind::CablePegboard),
            "icp" | "control-panel" | "industrial-control-panel" => Some(FurnitureKind::ControlPanel),
            _ => None,
        }
    }

    /// Rack units, shelf levels (a table has one place to stand things).
    pub fn default_size(self) -> u32 {
        match self {
            FurnitureKind::Rack => 12,
            FurnitureKind::Shelf => 4,
            FurnitureKind::Table => 1,
            FurnitureKind::InventoryShelf => 4,
            FurnitureKind::CablePegboard => 4,
            FurnitureKind::ControlPanel => 12,
        }
    }

    /// Whether it is laid out as a rack: numbered slots down it, an item taking several.
    pub fn is_rack_like(self) -> bool {
        matches!(self, FurnitureKind::Rack | FurnitureKind::ControlPanel)
    }

    /// Whether it is laid out as a shelf: levels holding a few items each.
    pub fn is_shelf_like(self) -> bool {
        matches!(self, FurnitureKind::Shelf | FurnitureKind::InventoryShelf)
    }

    /// What its size counts.
    pub fn size_unit(self) -> &'static str {
        match self {
            FurnitureKind::Rack => "units",
            FurnitureKind::ControlPanel => "DIN slots",
            FurnitureKind::CablePegboard => "rows of hooks",
            _ => "levels",
        }
    }

    /// The fewest and most units (levels) it can have.
    pub fn size_range(self) -> (u32, u32) {
        match self {
            FurnitureKind::Rack => (4, 48),
            FurnitureKind::Shelf => (1, 8),
            FurnitureKind::Table => (1, 1),
            FurnitureKind::InventoryShelf => (1, 8),
            FurnitureKind::CablePegboard => (1, 8),
            FurnitureKind::ControlPanel => (6, 24),
        }
    }
}

/// A rack, shelf or table in a wiring closet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Furniture {
    pub id: u32,
    /// The wiring closet it stands in.
    pub place: u32,
    pub kind: FurnitureKind,
    #[serde(default)]
    pub name: String,
    /// Where it stands in the closet (closet units).
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    /// Rack units or shelf levels.
    #[serde(default)]
    pub size: u32,
    /// A table's width in cells (0: as made, [`TABLE_COLS`]).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub width: u32,
    /// A table's depth in rows of cells (0: as made, [`TABLE_ROWS`]).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub depth: u32,
    /// A rack turned around: its back faces the viewer.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub back: bool,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl Furniture {
    /// A table's cells to a row.
    pub fn table_cols(&self) -> u32 {
        if self.width == 0 { TABLE_COLS } else { self.width }
    }

    /// A table's rows of cells.
    pub fn table_rows(&self) -> u32 {
        if self.depth == 0 { TABLE_ROWS } else { self.depth }
    }

    /// How many items a table holds.
    pub fn table_capacity(&self) -> usize {
        (self.table_cols() as usize).saturating_mul(self.table_rows() as usize)
    }
}

/// A passive device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FittingKind {
    CopperPatchPanel,
    FiberPatchPanel,
    /// A wall outlet for copper cabling: a device plugs into it, and the building's structured
    /// cabling carries the link to a patch panel or another wall mount.
    CopperWallMount,
    FiberWallMount,
    PowerStrip,
}

impl FittingKind {
    pub const ALL: [FittingKind; 5] = [
        FittingKind::CopperPatchPanel,
        FittingKind::FiberPatchPanel,
        FittingKind::CopperWallMount,
        FittingKind::FiberWallMount,
        FittingKind::PowerStrip,
    ];

    /// The four structured cabling items, in the order of the device box.
    pub const CABLING: [FittingKind; 4] =
        [FittingKind::CopperPatchPanel, FittingKind::FiberPatchPanel, FittingKind::CopperWallMount, FittingKind::FiberWallMount];

    pub fn id(self) -> &'static str {
        match self {
            FittingKind::CopperPatchPanel => "copper-patch-panel",
            FittingKind::FiberPatchPanel => "fiber-patch-panel",
            FittingKind::CopperWallMount => "copper-wall-mount",
            FittingKind::FiberWallMount => "fiber-wall-mount",
            FittingKind::PowerStrip => "power-strip",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FittingKind::CopperPatchPanel => "Copper Patch Panel",
            FittingKind::FiberPatchPanel => "Fiber Patch Panel",
            FittingKind::CopperWallMount => "Copper Wall Mount",
            FittingKind::FiberWallMount => "Fiber Wall Mount",
            FittingKind::PowerStrip => "Power Distribution Device",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            FittingKind::CopperPatchPanel => "Copper patch panel: joins device ports through the structured cabling",
            FittingKind::FiberPatchPanel => "Fiber patch panel: joins fiber device ports through the structured cabling",
            FittingKind::CopperWallMount => "Copper wall mount: a wall outlet that carries a link through the building's cabling",
            FittingKind::FiberWallMount => "Fiber wall mount: a fiber wall outlet that carries a link through the building's cabling",
            FittingKind::PowerStrip => "Power distribution device: a power strip; what is plugged into it follows its switch",
        }
    }

    pub fn from_id(s: &str) -> Option<FittingKind> {
        let k = s.trim().to_ascii_lowercase().replace(['_', ' '], "-");
        FittingKind::ALL.into_iter().find(|f| f.id() == k).or(match k.as_str() {
            "patch-panel" | "panel" => Some(FittingKind::CopperPatchPanel),
            "pdu" | "power" | "powerstrip" => Some(FittingKind::PowerStrip),
            _ => None,
        })
    }

    /// A wall mount: a wall outlet, which also stands on the walls of a building's rooms.
    pub fn is_wall_mount(self) -> bool {
        matches!(self, FittingKind::CopperWallMount | FittingKind::FiberWallMount)
    }

    /// A patch panel or a wall mount: it has ports that patch wires join.
    pub fn is_cabling(self) -> bool {
        self != FittingKind::PowerStrip
    }

    pub fn is_panel(self) -> bool {
        matches!(self, FittingKind::CopperPatchPanel | FittingKind::FiberPatchPanel)
    }

    /// Fiber or copper (a power strip is neither).
    pub fn is_fiber(self) -> bool {
        matches!(self, FittingKind::FiberPatchPanel | FittingKind::FiberWallMount)
    }

    /// Ports of a panel or wall mount, outlets of a strip, as made.
    pub fn default_ports(self) -> u32 {
        match self {
            FittingKind::CopperPatchPanel => 24,
            FittingKind::FiberPatchPanel => 12,
            FittingKind::CopperWallMount | FittingKind::FiberWallMount => 2,
            FittingKind::PowerStrip => 8,
        }
    }

    pub fn port_range(self) -> (u32, u32) {
        match self {
            FittingKind::CopperPatchPanel | FittingKind::FiberPatchPanel => (4, 48),
            FittingKind::CopperWallMount | FittingKind::FiberWallMount => (1, 4),
            FittingKind::PowerStrip => (2, 16),
        }
    }

    /// Whether a device port's medium can be wired to this kind (copper to copper, fiber to
    /// fiber; a combo port takes either).
    pub fn takes(self, medium: crate::cable::Medium) -> bool {
        use crate::cable::Medium;
        matches!((self.is_fiber(), medium), (_, Medium::Combo) | (true, Medium::Fiber) | (false, Medium::Copper))
    }
}

fn yes() -> bool {
    true
}

/// The breaker rating of a new power distribution device, in watts (a small 6.5 A strip at 230 V).
pub const DEFAULT_RATING_W: u32 = 1_500;
/// The breaker ratings a power distribution device may be set to: up to a 16 A circuit at 230 V.
pub const RATING_RANGE_W: (u32, u32) = (100, 3_680);

fn default_rating() -> u32 {
    DEFAULT_RATING_W
}

fn is_default_rating(w: &u32) -> bool {
    *w == DEFAULT_RATING_W
}

fn one() -> u32 {
    1
}

/// A patch panel or a power distribution device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fitting {
    pub id: u32,
    pub place: u32,
    pub kind: FittingKind,
    #[serde(default)]
    pub name: String,
    /// Where it stands when it is not on furniture.
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    /// Panel ports or outlets.
    #[serde(default)]
    pub ports: u32,
    /// A power distribution device's switch.
    #[serde(default = "yes")]
    pub on: bool,
    /// A power distribution device's breaker: the most watts the devices that are on may draw
    /// together (their nominal draws, `Device::nominal_watts`).
    #[serde(default = "default_rating", skip_serializing_if = "is_default_rating")]
    pub rating_w: u32,
    /// The breaker has tripped: the strip is off and its devices lost power. Switching it on
    /// again resets it, once the devices plugged in fit its rating.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tripped: bool,
}

/// What can stand on furniture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Item {
    Device(DeviceId),
    Fitting(u32),
}

/// Where an item stands on a piece of furniture: a rack's first unit (from the top, 0-based) and
/// how many it takes, a shelf's level (0-based, from the top), or a table (slot 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mount {
    pub item: Item,
    pub furniture: u32,
    #[serde(default)]
    pub slot: u32,
    #[serde(default = "one")]
    pub units: u32,
}

/// One end of a patch wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pole {
    /// A patch panel's port (0-based).
    Panel { fitting: u32, port: u32 },
    /// A device's port.
    Device(End),
}

/// A patch wire: a patch cord between two panel ports, or a drop from a panel port to a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchWire {
    pub a: Pole,
    pub b: Pole,
}

impl PatchWire {
    fn has(&self, p: Pole) -> bool {
        self.a == p || self.b == p
    }

    /// Whether two wires would use the same end of the same kind (a drop and a cord may share a
    /// panel port).
    fn clashes(&self, other: &PatchWire) -> bool {
        self.is_cord() == other.is_cord() && (other.has(self.a) || other.has(self.b))
    }

    pub fn is_cord(&self) -> bool {
        matches!((self.a, self.b), (Pole::Panel { .. }, Pole::Panel { .. }))
    }

    /// The device port of a drop.
    fn device_end(&self) -> Option<End> {
        match (self.a, self.b) {
            (Pole::Device(e), Pole::Panel { .. }) | (Pole::Panel { .. }, Pole::Device(e)) => Some(e),
            _ => None,
        }
    }
}

/// A device plugged into an outlet of a power distribution device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plug {
    pub device: DeviceId,
    pub strip: u32,
    pub outlet: u32,
}

/// A cable hung on a peg of a cable pegboard (named by its two ends, like a hand-set length).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hang {
    pub furniture: u32,
    /// The hook, counted row by row from 0.
    pub peg: u32,
    pub a: End,
    pub b: End,
}

fn is_cable(l: &Link, h: &Hang) -> bool {
    (l.a == h.a && l.b == h.b) || (l.a == h.b && l.b == h.a)
}

/// Two devices cabled through patch panels: a drop, a patch cord and a drop.
#[derive(Debug, Clone, PartialEq)]
pub struct PatchPath {
    pub a: End,
    pub b: End,
    pub wires: Vec<PatchWire>,
}

/// Whether `slot` and `units` fit furniture `f` next to the other mounts on it.
fn check_slot(f: &Furniture, others: &[Mount], slot: u32, units: u32) -> Result<()> {
    let bad = |m: String| Err(ModelError::Invalid(m));
    match f.kind {
        FurnitureKind::CablePegboard => return bad("devices cannot stand on a cable pegboard: hang cables on its pegs".into()),
        FurnitureKind::Rack | FurnitureKind::ControlPanel => {
            if units == 0 || units > MAX_ITEM_UNITS {
                return bad(format!("an item takes 1 to {MAX_ITEM_UNITS} rack units"));
            }
            let end = slot.saturating_add(units);
            if slot >= f.size || end > f.size {
                return bad(format!("units {} to {} do not fit: {} has {} units", slot.saturating_add(1), end, f.name, f.size));
            }
            if others.iter().any(|m| slot < m.slot.saturating_add(m.units) && m.slot < end) {
                return bad(format!("{} units {} to {} are taken", f.name, slot.saturating_add(1), end));
            }
        }
        FurnitureKind::Shelf | FurnitureKind::InventoryShelf => {
            if units != 1 {
                return bad("an item on a shelf takes one place".into());
            }
            if slot >= f.size {
                return bad(format!("{} has {} levels", f.name, f.size));
            }
            if others.iter().filter(|m| m.slot == slot).count() >= SHELF_CAPACITY {
                return bad(format!("level {} of {} is full", slot.saturating_add(1), f.name));
            }
        }
        FurnitureKind::Table => {
            if slot != 0 || units != 1 {
                return bad("a table has one place to stand things".into());
            }
            if others.len() >= f.table_capacity() {
                return bad(format!("{} is full", f.name));
            }
        }
    }
    Ok(())
}

impl Topology {
    pub fn furniture_piece(&self, id: u32) -> Option<&Furniture> {
        self.physical.furniture.iter().find(|f| f.id == id)
    }

    pub fn fitting(&self, id: u32) -> Option<&Fitting> {
        self.physical.fittings.iter().find(|f| f.id == id)
    }

    /// Furniture in a closet.
    pub fn furniture_in(&self, place: u32) -> Vec<Furniture> {
        self.physical.furniture.iter().filter(|f| f.place == place).cloned().collect()
    }

    /// Passive devices in a closet.
    pub fn fittings_in(&self, place: u32) -> Vec<Fitting> {
        self.physical.fittings.iter().filter(|f| f.place == place).cloned().collect()
    }

    /// Where an item stands, when it is on furniture.
    pub fn mount_of(&self, item: Item) -> Option<Mount> {
        self.physical.mounts.iter().find(|m| m.item == item).copied()
    }

    /// What stands on a piece of furniture, in slot order.
    pub fn mounts_on(&self, furniture: u32) -> Vec<Mount> {
        let mut v: Vec<Mount> = self.physical.mounts.iter().filter(|m| m.furniture == furniture).copied().collect();
        v.sort_by_key(|m| m.slot);
        v
    }

    /// The closet an item stands in.
    pub fn item_place(&self, item: Item) -> Option<u32> {
        match item {
            Item::Device(d) => self.home_of(d).map(|h| h.place),
            Item::Fitting(i) => self.fitting(i).map(|f| f.place),
        }
    }

    /// An item's name.
    pub fn item_name(&self, item: Item) -> String {
        match item {
            Item::Device(d) => self.device(d).map(|x| x.name.clone()).unwrap_or_default(),
            Item::Fitting(i) => self.fitting(i).map(|f| f.name.clone()).unwrap_or_default(),
        }
    }

    fn item_exists(&self, item: Item) -> bool {
        match item {
            Item::Device(d) => self.device(d).is_some(),
            Item::Fitting(i) => self.fitting(i).is_some(),
        }
    }

    fn building_ids(&self) -> BTreeSet<u32> {
        self.places().iter().filter(|p| p.kind == PlaceKind::Building).map(|p| p.id).collect()
    }

    fn closet_ids(&self) -> BTreeSet<u32> {
        self.places().iter().filter(|p| p.kind.holds_things()).map(|p| p.id).collect()
    }

    /// Whether a passive device of this kind may stand in `place`: a wiring closet or a container,
    /// and for a wall mount a building too (its rooms).
    fn fitting_place_of(&self, kind: FittingKind, place: u32) -> Result<()> {
        let p = self.place(place).ok_or_else(|| ModelError::Invalid(format!("no place {place}")))?;
        if p.kind.holds_things() || (p.kind == PlaceKind::Building && kind.is_wall_mount()) {
            return Ok(());
        }
        Err(ModelError::Invalid("passive devices stand in a wiring closet or a container (a wall mount in a building too)".into()))
    }

    fn closet_of(&self, place: u32) -> Result<()> {
        let p = self.place(place).ok_or_else(|| ModelError::Invalid(format!("no place {place}")))?;
        if !p.kind.holds_things() {
            return Err(ModelError::Invalid("furniture and passive devices stand in a wiring closet or a container".into()));
        }
        Ok(())
    }

    /// Adds a rack, shelf or table to a wiring closet; returns its id.
    pub fn add_furniture(&mut self, kind: FurnitureKind, name: Option<&str>, place: u32, x: f32, y: f32, size: Option<u32>) -> Result<u32> {
        self.closet_of(place)?;
        if self.physical.furniture.len() >= MAX_FURNITURE {
            return Err(ModelError::Invalid(format!("at most {MAX_FURNITURE} pieces of furniture")));
        }
        let (lo, hi) = kind.size_range();
        let size = size.unwrap_or(kind.default_size());
        if !(lo..=hi).contains(&size) {
            return Err(ModelError::Invalid(format!("a {} has {lo} to {hi} {}", kind.label().to_lowercase(), kind.size_unit())));
        }
        self.materialize_places();
        let id = self.physical.furniture.iter().map(|f| f.id).max().unwrap_or(0).saturating_add(1);
        let n = self.physical.furniture.iter().filter(|f| f.kind == kind).count();
        let name = name.and_then(clean_name).unwrap_or_else(|| format!("{}{n}", kind.label()));
        self.physical.furniture.push(Furniture { id, place, kind, name, x: finite(x), y: finite(y), size, width: 0, depth: 0, back: false });
        Ok(id)
    }

    fn furniture_mut(&mut self, id: u32) -> Result<&mut Furniture> {
        self.physical.furniture.iter_mut().find(|f| f.id == id).ok_or_else(|| ModelError::Invalid(format!("no furniture {id}")))
    }

    pub fn rename_furniture(&mut self, id: u32, name: &str) -> Result<()> {
        let name = clean_name(name).ok_or_else(|| ModelError::Invalid("furniture needs a name".into()))?;
        self.furniture_mut(id)?.name = name;
        Ok(())
    }

    /// Gives a rack more or fewer units, or a shelf more or fewer levels. Refused when what
    /// stands on it would no longer fit (take it off, or move it up, first). A table has one size.
    pub fn resize_furniture(&mut self, id: u32, size: u32) -> Result<()> {
        let f = self.furniture_piece(id).cloned().ok_or_else(|| ModelError::Invalid(format!("no furniture {id}")))?;
        let (lo, hi) = f.kind.size_range();
        if lo == hi {
            return Err(ModelError::Invalid(format!("a {} has one size", f.kind.label().to_lowercase())));
        }
        if !(lo..=hi).contains(&size) {
            return Err(ModelError::Invalid(format!(
                "a {} has {lo} to {hi} {}",
                f.kind.label().to_lowercase(),
                if f.kind == FurnitureKind::Rack { "units" } else { "levels" }
            )));
        }
        if let Some(m) = self.mounts_on(id).iter().find(|m| m.slot.saturating_add(m.units.max(1)) > size) {
            let at = if f.kind == FurnitureKind::Rack {
                format!("unit {} ({} units)", m.slot.saturating_add(1), m.units)
            } else {
                format!("level {}", m.slot.saturating_add(1))
            };
            return Err(ModelError::Invalid(format!("{} stands at {at}: take it off or move it up first", self.item_name(m.item))));
        }
        self.furniture_mut(id)?.size = size;
        Ok(())
    }

    /// Gives a table another width (cells in a row) and depth (rows). Refused when more items
    /// stand on it than the new table holds (take some off first).
    pub fn resize_table(&mut self, id: u32, width: Option<u32>, depth: Option<u32>) -> Result<()> {
        let f = self.furniture_piece(id).cloned().ok_or_else(|| ModelError::Invalid(format!("no furniture {id}")))?;
        if f.kind != FurnitureKind::Table {
            return Err(ModelError::Invalid("only a table has a width and a depth".into()));
        }
        if width.is_none() && depth.is_none() {
            return Err(ModelError::Invalid("a width or a depth is required".into()));
        }
        let (w, d) = (width.unwrap_or(f.table_cols()), depth.unwrap_or(f.table_rows()));
        let ((wlo, whi), (dlo, dhi)) = (TABLE_WIDTH_RANGE, TABLE_DEPTH_RANGE);
        if !(wlo..=whi).contains(&w) {
            return Err(ModelError::Invalid(format!("a table is {wlo} to {whi} cells wide")));
        }
        if !(dlo..=dhi).contains(&d) {
            return Err(ModelError::Invalid(format!("a table is {dlo} to {dhi} cells deep")));
        }
        let held = self.mounts_on(id).len();
        if held > (w as usize).saturating_mul(d as usize) {
            return Err(ModelError::Invalid(format!("{held} items stand on {}: take some off first", f.name)));
        }
        let t = self.furniture_mut(id)?;
        t.width = if w == TABLE_COLS { 0 } else { w };
        t.depth = if d == TABLE_ROWS { 0 } else { d };
        Ok(())
    }

    /// Turns a rack around (`back`) or to its front again; none flips it. Returns whether its
    /// back faces the viewer now. Only a rack has a back.
    pub fn turn_furniture(&mut self, id: u32, back: Option<bool>) -> Result<bool> {
        let f = self.furniture_mut(id)?;
        if f.kind != FurnitureKind::Rack {
            return Err(ModelError::Invalid("only a rack can be turned around".into()));
        }
        f.back = back.unwrap_or(!f.back);
        Ok(f.back)
    }

    /// Whether the loose items of a closet are laid out as a list (one to a row) rather than a grid.
    pub fn floor_is_list(&self, place: u32) -> bool {
        self.physical.floor_lists.contains(&place)
    }

    /// Lays the loose items of a closet out as a list or as a grid.
    pub fn set_floor_list(&mut self, place: u32, list: bool) -> Result<()> {
        self.closet_of(place)?;
        self.materialize_places();
        self.physical.floor_lists.retain(|p| *p != place);
        if list {
            self.physical.floor_lists.push(place);
            self.physical.floor_lists.sort_unstable();
        }
        Ok(())
    }

    pub fn move_furniture(&mut self, id: u32, x: f32, y: f32) -> Result<()> {
        let f = self.furniture_mut(id)?;
        f.x = finite(x);
        f.y = finite(y);
        Ok(())
    }

    /// Removes furniture; what stood on it stays in the closet, loose.
    pub fn remove_furniture(&mut self, id: u32) -> Result<Furniture> {
        let f = self.furniture_piece(id).cloned().ok_or_else(|| ModelError::Invalid(format!("no furniture {id}")))?;
        self.physical.furniture.retain(|x| x.id != id);
        self.physical.mounts.retain(|m| m.furniture != id);
        self.physical.hangs.retain(|h| h.furniture != id);
        Ok(f)
    }

    /// The first slot where an item of `units` units fits on a piece of furniture.
    pub fn first_free_slot(&self, item: Item, furniture: u32, units: u32) -> Option<u32> {
        let f = self.furniture_piece(furniture)?;
        let others: Vec<Mount> = self.physical.mounts.iter().filter(|m| m.furniture == furniture && m.item != item).copied().collect();
        let slots = if f.kind == FurnitureKind::Table { 1 } else { f.size };
        if f.kind == FurnitureKind::CablePegboard {
            return None;
        }
        (0..slots).find(|s| check_slot(f, &others, *s, units).is_ok())
    }

    /// Puts an item on furniture (moving it into the furniture's closet), at `slot` (else the
    /// first free one) taking `units` rack units. An item already on furniture is moved.
    pub fn mount_item(&mut self, item: Item, furniture: u32, slot: Option<u32>, units: Option<u32>) -> Result<Mount> {
        let f = self.furniture_piece(furniture).cloned().ok_or_else(|| ModelError::Invalid(format!("no furniture {furniture}")))?;
        match item {
            Item::Device(d) => {
                if self.device(d).is_none() {
                    return Err(ModelError::NoDevice(d.to_string()));
                }
            }
            Item::Fitting(i) => {
                if self.fitting(i).is_none() {
                    return Err(ModelError::Invalid(format!("no passive device {i}")));
                }
            }
        }
        self.check_accepts(&f, item)?;
        let units = units.unwrap_or(1);
        let slot = match slot {
            Some(s) => s,
            None => self.first_free_slot(item, furniture, units).ok_or_else(|| ModelError::Invalid(format!("{} has no room", f.name)))?,
        };
        let others: Vec<Mount> = self.physical.mounts.iter().filter(|m| m.furniture == furniture && m.item != item).copied().collect();
        check_slot(&f, &others, slot, units)?;
        match item {
            Item::Device(d) => {
                if self.home_of(d).map(|h| h.place) != Some(f.place) {
                    self.set_home(d, f.place, f.x, f.y)?;
                }
            }
            Item::Fitting(i) => {
                if let Some(x) = self.physical.fittings.iter_mut().find(|x| x.id == i) {
                    x.place = f.place;
                }
            }
        }
        self.materialize_places();
        self.physical.mounts.retain(|m| m.item != item);
        let m = Mount { item, furniture, slot, units };
        self.physical.mounts.push(m);
        self.reconcile_places();
        // Stored devices are unpowered. Taken out to a rack, a shelf or a table, a device keeps
        // the power state it has: it stays off until someone switches it on.
        if f.kind == FurnitureKind::InventoryShelf
            && let Item::Device(d) = item
            && let Some(dev) = self.device_mut(d)
            && !dev.powered_off
        {
            dev.power_off();
        }
        Ok(m)
    }

    /// Whether furniture takes an item at all: an industrial control panel takes industrial
    /// devices only.
    fn check_accepts(&self, f: &Furniture, item: Item) -> Result<()> {
        if f.kind != FurnitureKind::ControlPanel {
            return Ok(());
        }
        match item {
            Item::Device(d) => {
                let model = self.device(d).map(|x| x.model.clone()).unwrap_or_default();
                if icp_accepts(&model) {
                    Ok(())
                } else {
                    Err(ModelError::Invalid(format!(
                        "{} is not industrial equipment: a control panel's DIN rail takes PLCs, HMIs, industrial switches and routers, signal generators and power supplies",
                        self.item_name(item)
                    )))
                }
            }
            Item::Fitting(_) => Err(ModelError::Invalid("a control panel's DIN rail takes industrial devices only".into())),
        }
    }

    /// The pegs of a cable pegboard.
    pub fn peg_count(&self, furniture: u32) -> u32 {
        self.furniture_piece(furniture).filter(|f| f.kind == FurnitureKind::CablePegboard).map_or(0, |f| f.size.saturating_mul(PEGS_PER_ROW))
    }

    /// The cables hung on a pegboard, by peg.
    pub fn hangs_on(&self, furniture: u32) -> Vec<Hang> {
        let mut v: Vec<Hang> = self.physical.hangs.iter().filter(|h| h.furniture == furniture).copied().collect();
        v.sort_by_key(|h| h.peg);
        v
    }

    /// Where the cable at `end` hangs.
    pub fn hang_of(&self, end: End) -> Option<Hang> {
        self.physical.hangs.iter().find(|h| h.a == end || h.b == end).copied()
    }

    /// Hangs the cable at `end` on a peg (else the first free one) of a pegboard standing in the
    /// closet of one of its ends. A cable hangs on one peg: it moves from where it was.
    pub fn hang_cable(&mut self, furniture: u32, end: End, peg: Option<u32>) -> Result<Hang> {
        let f = self.furniture_piece(furniture).cloned().ok_or_else(|| ModelError::Invalid(format!("no furniture {furniture}")))?;
        if f.kind != FurnitureKind::CablePegboard {
            return Err(ModelError::Invalid(format!("{} has no pegs: cables hang on a cable pegboard", f.name)));
        }
        let l = *self.link_at(end).ok_or_else(|| ModelError::Invalid(format!("{} has no cable", self.end_label(end))))?;
        let here = |e: End| self.home_of(e.device).map(|h| h.place) == Some(f.place);
        if !here(l.a) && !here(l.b) {
            return Err(ModelError::Invalid(format!("{} stands in another place: hang cables on a pegboard where one of their ends is", f.name)));
        }
        let pegs = self.peg_count(furniture);
        let taken: BTreeSet<u32> = self.physical.hangs.iter().filter(|h| h.furniture == furniture && !is_cable(&l, h)).map(|h| h.peg).collect();
        let peg = match peg {
            Some(p) => p,
            None => (0..pegs).find(|p| !taken.contains(p)).ok_or_else(|| ModelError::Invalid(format!("{} has no free peg", f.name)))?,
        };
        if peg >= pegs {
            return Err(ModelError::Invalid(format!("{} has {pegs} pegs", f.name)));
        }
        if taken.contains(&peg) {
            return Err(ModelError::Invalid(format!("peg {} of {} is taken", peg.saturating_add(1), f.name)));
        }
        if self.physical.hangs.len() >= MAX_HANGS && !self.physical.hangs.iter().any(|h| is_cable(&l, h)) {
            return Err(ModelError::Invalid(format!("at most {MAX_HANGS} hung cables")));
        }
        self.materialize_places();
        self.physical.hangs.retain(|h| !is_cable(&l, h));
        let h = Hang { furniture, peg, a: l.a, b: l.b };
        self.physical.hangs.push(h);
        Ok(h)
    }

    /// Takes the cable at `end` off its peg; whether it was hung.
    pub fn unhang_cable(&mut self, end: End) -> bool {
        let before = self.physical.hangs.len();
        self.physical.hangs.retain(|h| h.a != end && h.b != end);
        self.physical.hangs.len() != before
    }

    /// Takes an item off its furniture; it stays in the closet. Whether it was on furniture.
    pub fn unmount_item(&mut self, item: Item) -> bool {
        let before = self.physical.mounts.len();
        self.physical.mounts.retain(|m| m.item != item);
        self.physical.mounts.len() != before
    }

    /// Adds a patch panel or a power distribution device to a closet; returns its id.
    pub fn add_fitting(&mut self, kind: FittingKind, name: Option<&str>, place: u32, x: f32, y: f32, ports: Option<u32>) -> Result<u32> {
        self.fitting_place_of(kind, place)?;
        if self.physical.fittings.len() >= MAX_FITTINGS {
            return Err(ModelError::Invalid(format!("at most {MAX_FITTINGS} passive devices")));
        }
        let (lo, hi) = kind.port_range();
        let ports = ports.unwrap_or(kind.default_ports());
        if !(lo..=hi).contains(&ports) {
            return Err(ModelError::Invalid(format!(
                "a {} has {lo} to {hi} {}",
                kind.label().to_lowercase(),
                if kind.is_cabling() { "ports" } else { "outlets" }
            )));
        }
        self.materialize_places();
        let id = self.physical.fittings.iter().map(|f| f.id).max().unwrap_or(0).saturating_add(1);
        let n = self.physical.fittings.iter().filter(|f| f.kind == kind).count();
        let name = name.and_then(clean_name).unwrap_or_else(|| format!("{}{n}", kind.label()));
        self.physical.fittings.push(Fitting {
            id,
            place,
            kind,
            name,
            x: finite(x),
            y: finite(y),
            ports,
            on: true,
            rating_w: DEFAULT_RATING_W,
            tripped: false,
        });
        Ok(id)
    }

    fn fitting_mut(&mut self, id: u32) -> Result<&mut Fitting> {
        self.physical.fittings.iter_mut().find(|f| f.id == id).ok_or_else(|| ModelError::Invalid(format!("no passive device {id}")))
    }

    pub fn rename_fitting(&mut self, id: u32, name: &str) -> Result<()> {
        let name = clean_name(name).ok_or_else(|| ModelError::Invalid("a passive device needs a name".into()))?;
        self.fitting_mut(id)?.name = name;
        Ok(())
    }

    pub fn move_fitting(&mut self, id: u32, x: f32, y: f32) -> Result<()> {
        let f = self.fitting_mut(id)?;
        f.x = finite(x);
        f.y = finite(y);
        Ok(())
    }

    /// Removes a passive device with its wires (a cable they carried is unplugged), its plugs and
    /// its place on furniture.
    pub fn remove_fitting(&mut self, id: u32) -> Result<Fitting> {
        let f = self.fitting(id).cloned().ok_or_else(|| ModelError::Invalid(format!("no passive device {id}")))?;
        let poles: Vec<Pole> = self
            .physical
            .patch_wires
            .iter()
            .filter(|w| matches!(w.a, Pole::Panel { fitting, .. } if fitting == id) || matches!(w.b, Pole::Panel { fitting, .. } if fitting == id))
            .flat_map(|w| [w.a, w.b])
            .filter(|p| matches!(p, Pole::Panel { fitting, .. } if *fitting == id))
            .collect();
        for p in poles {
            self.remove_patch_wire(p, None);
        }
        self.physical.fittings.retain(|x| x.id != id);
        self.physical.mounts.retain(|m| m.item != Item::Fitting(id));
        self.physical.plugs.retain(|p| p.strip != id);
        Ok(f)
    }

    /// The closet a wire end is in.
    pub fn pole_place(&self, p: Pole) -> Option<u32> {
        match p {
            Pole::Panel { fitting, .. } => self.fitting(fitting).map(|f| f.place),
            Pole::Device(e) => self.home_of(e.device).map(|h| h.place),
        }
    }

    /// Whether a wire end names a real panel port or device port.
    fn pole_ok(&self, p: Pole) -> bool {
        match p {
            Pole::Panel { fitting, port } => self.fitting(fitting).is_some_and(|f| f.kind.is_cabling() && port < f.ports),
            Pole::Device(e) => self.device(e.device).is_some_and(|d| e.port < d.ports.len()),
        }
    }

    /// `Panel0 port 3` or `Switch0 Fa0/1`, for messages.
    pub fn pole_label(&self, p: Pole) -> String {
        match p {
            Pole::Panel { fitting, port } => format!("{} {}", self.fitting(fitting).map(|f| f.name.as_str()).unwrap_or("?"), port.saturating_add(1)),
            Pole::Device(e) => self.end_label(e),
        }
    }

    /// The patch wires at a wire end (a panel port can have a drop and a cord).
    pub fn patch_wires_at(&self, p: Pole) -> Vec<PatchWire> {
        self.physical.patch_wires.iter().filter(|w| w.has(p)).copied().collect()
    }

    /// Joins two wire ends: a panel port to another panel port, or a panel port to a free device
    /// port. Both ends stand in the same closet and are free. When this completes a patch path
    /// between two free device ports, they are cabled (an automatic cable); the ends of the
    /// cables made are returned.
    pub fn add_patch_wire(&mut self, a: Pole, b: Pole) -> Result<Vec<End>> {
        self.add_wire(a, b, false)
    }

    /// Joins a panel or wall mount port to one in another place with a backbone cable. When this
    /// completes a path between two free device ports (a drop at each end), they are cabled; the
    /// ends of the cables made are returned.
    pub fn add_backbone(&mut self, a: Pole, b: Pole) -> Result<Vec<End>> {
        self.add_wire(a, b, true)
    }

    fn add_wire(&mut self, a: Pole, b: Pole, backbone: bool) -> Result<Vec<End>> {
        if a == b {
            return Err(ModelError::Invalid("a patch wire joins two different ports".into()));
        }
        if matches!((a, b), (Pole::Device(_), Pole::Device(_))) {
            return Err(ModelError::Invalid("two device ports are joined with a cable, not a patch wire".into()));
        }
        let wire = PatchWire { a, b };
        for p in [a, b] {
            if !self.pole_ok(p) {
                return Err(ModelError::Invalid(format!("{} is not a patch panel port or a device port", self.pole_label(p))));
            }
            if let Pole::Device(e) = p
                && self.link_at(e).is_some()
            {
                return Err(ModelError::Invalid(format!("{} already has a cable", self.end_label(e))));
            }
        }
        self.check_media(a, b)?;
        if let Some(p) = [a, b].into_iter().find(|p| self.physical.patch_wires.iter().any(|w| w.clashes(&wire) && w.has(*p))) {
            return Err(ModelError::Invalid(format!("{} is already wired", self.pole_label(p))));
        }
        let (place_a, place_b) = (self.pole_place(a), self.pole_place(b));
        if backbone {
            if !wire.is_cord() {
                return Err(ModelError::Invalid("a backbone joins two patch panel or wall mount ports".into()));
            }
            if place_a == place_b {
                return Err(ModelError::Invalid("a backbone joins two different places: a patch cord joins ports in one closet".into()));
            }
        } else if place_a != place_b {
            return Err(ModelError::Invalid(if wire.is_cord() {
                "both ends of a patch wire stand in the same wiring closet: a cable to another place is a backbone".into()
            } else {
                "both ends of a patch wire stand in the same wiring closet".into()
            }));
        }
        if self.physical.patch_wires.len() >= MAX_PATCH_WIRES {
            return Err(ModelError::Invalid(format!("at most {MAX_PATCH_WIRES} patch wires")));
        }
        self.materialize_places();
        self.physical.patch_wires.push(wire);
        let mut made = Vec::new();
        for path in self.patch_paths().into_iter().filter(|p| p.wires.contains(&wire)) {
            if self.link_at(path.a).is_some() || self.link_at(path.b).is_some() {
                continue;
            }
            if let Err(e) = self.connect(path.a, path.b) {
                self.physical.patch_wires.retain(|w| *w != wire);
                for end in made {
                    self.disconnect(end);
                }
                return Err(e);
            }
            made.push(path.a);
        }
        Ok(made)
    }

    /// Copper goes to copper and fiber to fiber: a drop needs a device port of the panel's
    /// medium, a cord two panels or wall mounts of one medium.
    fn check_media(&self, a: Pole, b: Pole) -> Result<()> {
        let kind = |p: Pole| match p {
            Pole::Panel { fitting, .. } => self.fitting(fitting).map(|f| f.kind),
            Pole::Device(_) => None,
        };
        match (a, b) {
            (Pole::Panel { .. }, Pole::Panel { .. }) => {
                if let (Some(x), Some(y)) = (kind(a), kind(b))
                    && x.is_fiber() != y.is_fiber()
                {
                    return Err(ModelError::Invalid(format!(
                        "{} and {} are not the same medium: copper joins copper, fiber joins fiber",
                        x.label(),
                        y.label()
                    )));
                }
            }
            (Pole::Device(e), Pole::Panel { .. }) | (Pole::Panel { .. }, Pole::Device(e)) => {
                let panel = if matches!(a, Pole::Panel { .. }) { kind(a) } else { kind(b) };
                let medium = self.device(e.device).and_then(|d| d.port_medium(e.port));
                if let (Some(k), Some(m)) = (panel, medium)
                    && !k.takes(m)
                {
                    return Err(ModelError::Invalid(format!(
                        "{} is {}: it does not go on a {}",
                        self.end_label(e),
                        m.words(),
                        k.label().to_lowercase()
                    )));
                }
            }
            (Pole::Device(_), Pole::Device(_)) => {}
        }
        Ok(())
    }

    /// Removes the patch wire between `a` and `b`, or with no `b` every wire at `a`; a cable a
    /// removed wire's path carried is unplugged. Whether a wire was removed.
    pub fn remove_patch_wire(&mut self, a: Pole, b: Option<Pole>) -> bool {
        let gone: Vec<PatchWire> = self.physical.patch_wires.iter().filter(|w| w.has(a) && b.is_none_or(|b| w.has(b))).copied().collect();
        self.remove_wires(gone)
    }

    /// Removes the backbone at `a` (or the one between `a` and `b`); drops and patch cords at the
    /// port stay. Whether a backbone was removed.
    pub fn remove_backbone(&mut self, a: Pole, b: Option<Pole>) -> bool {
        let gone: Vec<PatchWire> = self.backbones().into_iter().filter(|w| w.has(a) && b.is_none_or(|b| w.has(b))).collect();
        self.remove_wires(gone)
    }

    fn remove_wires(&mut self, gone: Vec<PatchWire>) -> bool {
        if gone.is_empty() {
            return false;
        }
        let paths: Vec<PatchPath> = self.patch_paths().into_iter().filter(|x| x.wires.iter().any(|w| gone.contains(w))).collect();
        self.physical.patch_wires.retain(|w| !gone.contains(w));
        for path in paths {
            if self.link_at(path.a).is_some_and(|l| (l.a == path.a && l.b == path.b) || (l.a == path.b && l.b == path.a)) {
                self.disconnect(path.a);
            }
        }
        true
    }

    /// Whether a patch wire is a backbone: a cord whose two ends stand in different places.
    pub fn is_backbone(&self, w: &PatchWire) -> bool {
        w.is_cord() && self.pole_place(w.a) != self.pole_place(w.b)
    }

    /// Every backbone cable of the lab.
    pub fn backbones(&self) -> Vec<PatchWire> {
        self.physical.patch_wires.iter().filter(|w| self.is_backbone(w)).copied().collect()
    }

    /// The backbone that goes from a port of this place, if any: the port's own end first.
    pub fn backbone_at(&self, p: Pole) -> Option<(Pole, Pole)> {
        self.backbones().into_iter().find(|w| w.has(p)).map(|w| if w.a == p { (w.a, w.b) } else { (w.b, w.a) })
    }

    /// The first port of a panel or wall mount that no patch cord or backbone is wired to.
    pub fn first_free_cord_port(&self, fitting: u32) -> Option<u32> {
        let f = self.fitting(fitting).filter(|f| f.kind.is_cabling())?;
        (0..f.ports).find(|port| {
            let p = Pole::Panel { fitting, port: *port };
            self.physical.patch_wires.iter().all(|w| !(w.is_cord() && w.has(p)))
        })
    }

    /// The backbone a cable runs through, as `(near end, far end)` panel ports in the order of the
    /// cable's two ends (none: the cable does not go through a backbone).
    pub fn backbone_of_link(&self, l: &Link) -> Option<(Pole, Pole)> {
        let path = self.patch_paths().into_iter().find(|p| (p.a == l.a && p.b == l.b) || (p.a == l.b && p.b == l.a))?;
        let cord = path.wires.iter().find(|w| self.is_backbone(w))?;
        Some((cord.a, cord.b))
    }

    /// Every pair of devices cabled through patch panels: a drop, a patch cord and a drop.
    pub fn patch_paths(&self) -> Vec<PatchPath> {
        let wires = &self.physical.patch_wires;
        let drop_at = |p: Pole| wires.iter().find(|w| w.has(p) && w.device_end().is_some());
        let mut out = Vec::new();
        for cord in wires.iter().filter(|w| w.is_cord()) {
            let (Some(wa), Some(wb)) = (drop_at(cord.a), drop_at(cord.b)) else { continue };
            let (Some(a), Some(b)) = (wa.device_end(), wb.device_end()) else { continue };
            out.push(PatchPath { a, b, wires: vec![*wa, *cord, *wb] });
        }
        out
    }

    /// Whether a cable is carried by a patch path (it is drawn as the wires of the path).
    pub fn link_is_patched(&self, l: &Link) -> bool {
        self.patch_paths().iter().any(|p| (p.a == l.a && p.b == l.b) || (p.a == l.b && p.b == l.a))
    }

    /// Plugs a device into an outlet (else the first free one) of a power distribution device in
    /// its closet; a device has one plug.
    pub fn plug_in(&mut self, device: DeviceId, strip: u32, outlet: Option<u32>) -> Result<Plug> {
        let dev = self.device(device).ok_or_else(|| ModelError::NoDevice(device.to_string()))?;
        let name = dev.name.clone();
        let f = self.fitting(strip).cloned().ok_or_else(|| ModelError::Invalid(format!("no passive device {strip}")))?;
        if f.kind != FittingKind::PowerStrip {
            return Err(ModelError::Invalid(format!("{} has no outlets", f.name)));
        }
        if self.home_of(device).map(|h| h.place) != Some(f.place) {
            return Err(ModelError::Invalid(format!("{name} and {} stand in different wiring closets", f.name)));
        }
        let taken = |o: u32| self.physical.plugs.iter().any(|p| p.strip == strip && p.outlet == o && p.device != device);
        let outlet = match outlet {
            Some(o) if o < f.ports && !taken(o) => o,
            Some(o) if o < f.ports => return Err(ModelError::Invalid(format!("outlet {} of {} is in use", o.saturating_add(1), f.name))),
            Some(_) => return Err(ModelError::Invalid(format!("{} has {} outlets", f.name, f.ports))),
            None => (0..f.ports).find(|o| !taken(*o)).ok_or_else(|| ModelError::Invalid(format!("{} has no free outlet", f.name)))?,
        };
        self.materialize_places();
        self.physical.plugs.retain(|p| p.device != device);
        let plug = Plug { device, strip, outlet };
        self.physical.plugs.push(plug);
        Ok(plug)
    }

    /// Pulls a device's plug out; whether it was plugged in.
    pub fn unplug(&mut self, device: DeviceId) -> bool {
        let before = self.physical.plugs.len();
        self.physical.plugs.retain(|p| p.device != device);
        self.physical.plugs.len() != before
    }

    /// The plug of a device.
    pub fn plug_of(&self, device: DeviceId) -> Option<Plug> {
        self.physical.plugs.iter().find(|p| p.device == device).copied()
    }

    /// What is plugged into a power distribution device.
    pub fn plugs_in(&self, strip: u32) -> Vec<Plug> {
        let mut v: Vec<Plug> = self.physical.plugs.iter().filter(|p| p.strip == strip).copied().collect();
        v.sort_by_key(|p| p.outlet);
        v
    }

    /// Whether a device is plugged into a power distribution device that is switched off.
    pub fn cut_by_strip(&self, device: DeviceId) -> bool {
        self.plug_of(device).and_then(|p| self.fitting(p.strip)).is_some_and(|f| !f.on)
    }

    /// The nominal draw, in watts, of the devices plugged into a power distribution device that
    /// are switched on: what its breaker carries.
    pub fn strip_load(&self, strip: u32) -> u32 {
        self.plugs_in(strip)
            .iter()
            .filter_map(|p| self.device(p.device))
            .filter(|d| !d.powered_off)
            .fold(0u32, |sum, d| sum.saturating_add(d.nominal_watts()))
    }

    /// The nominal draw of everything plugged into a power distribution device, switched on or
    /// not: what the breaker would carry if all of it were on.
    pub fn strip_demand(&self, strip: u32) -> u32 {
        self.plugs_in(strip).iter().filter_map(|p| self.device(p.device)).fold(0u32, |sum, d| sum.saturating_add(d.nominal_watts()))
    }

    /// Switches a power distribution device; returns the devices plugged into it. A strip whose
    /// breaker tripped comes back only when what is plugged into it fits its rating.
    pub fn set_strip_power(&mut self, strip: u32, on: bool) -> Result<Vec<DeviceId>> {
        let demand = self.strip_demand(strip);
        let f = self.fitting_mut(strip)?;
        if f.kind != FittingKind::PowerStrip {
            return Err(ModelError::Invalid(format!("{} has no power switch", f.name)));
        }
        if on && f.tripped {
            if demand > f.rating_w {
                return Err(ModelError::Invalid(format!(
                    "the breaker of {} tripped and {demand} W is still plugged in over its {} W rating: unplug something first",
                    f.name, f.rating_w
                )));
            }
            f.tripped = false;
        }
        f.on = on;
        Ok(self.plugs_in(strip).iter().map(|p| p.device).collect())
    }

    /// Sets the breaker rating of a power distribution device, in watts (100 to 3680).
    pub fn set_strip_rating(&mut self, strip: u32, watts: u32) -> Result<()> {
        let (lo, hi) = RATING_RANGE_W;
        let f = self.fitting_mut(strip)?;
        if f.kind != FittingKind::PowerStrip {
            return Err(ModelError::Invalid(format!("{} has no breaker", f.name)));
        }
        if !(lo..=hi).contains(&watts) {
            return Err(ModelError::Invalid(format!("a breaker rating is {lo} to {hi} W")));
        }
        f.rating_w = watts;
        Ok(())
    }

    /// Whether the devices that are on draw more than the breaker of a switched-on power
    /// distribution device carries.
    pub fn strip_overloaded(&self, strip: u32) -> bool {
        self.fitting(strip).is_some_and(|f| f.kind == FittingKind::PowerStrip && f.on && self.strip_load(strip) > f.rating_w)
    }

    /// Trips the breaker: the strip switches off; returns the devices plugged into it.
    pub fn trip_strip(&mut self, strip: u32) -> Vec<DeviceId> {
        if let Some(f) = self.physical.fittings.iter_mut().find(|f| f.id == strip && f.kind == FittingKind::PowerStrip) {
            f.on = false;
            f.tripped = true;
        }
        self.plugs_in(strip).iter().map(|p| p.device).collect()
    }

    /// Drops what no longer fits: an item on furniture in another closet, a plug into a strip in
    /// another closet, a patch wire across closets.
    pub(crate) fn reconcile_places(&mut self) {
        let keep: Vec<bool> = self
            .physical
            .mounts
            .iter()
            .map(|m| {
                let fp = self.furniture_piece(m.furniture).map(|f| f.place);
                fp.is_some() && fp == self.item_place(m.item)
            })
            .collect();
        let mut it = keep.into_iter();
        self.physical.mounts.retain(|_| it.next().unwrap_or(false));
        let keep: Vec<bool> = self
            .physical
            .plugs
            .iter()
            .map(|p| {
                let sp = self.fitting(p.strip).map(|f| f.place);
                sp.is_some() && sp == self.home_of(p.device).map(|h| h.place)
            })
            .collect();
        let mut it = keep.into_iter();
        self.physical.plugs.retain(|_| it.next().unwrap_or(false));
        let moved: Vec<PatchWire> = self
            .physical
            .patch_wires
            .iter()
            .filter(|w| {
                self.pole_place(w.a).is_none() || self.pole_place(w.b).is_none() || (!w.is_cord() && self.pole_place(w.a) != self.pole_place(w.b))
            })
            .copied()
            .collect();
        for w in moved {
            self.remove_patch_wire(w.a, Some(w.b));
        }
    }

    /// Forgets what refers to places, devices or passive devices that are gone.
    pub(crate) fn prune_furniture(&mut self) {
        let closets = self.closet_ids();
        let buildings = self.building_ids();
        self.physical.furniture.retain(|f| closets.contains(&f.place));
        self.physical.floor_lists.retain(|p| closets.contains(p));
        self.physical.fittings.retain(|f| closets.contains(&f.place) || (f.kind.is_wall_mount() && buildings.contains(&f.place)));
        let mounts: Vec<Mount> =
            self.physical.mounts.iter().filter(|m| self.item_exists(m.item) && self.furniture_piece(m.furniture).is_some()).copied().collect();
        self.physical.mounts = mounts;
        let plugs: Vec<Plug> = self
            .physical
            .plugs
            .iter()
            .filter(|p| {
                self.device(p.device).is_some() && self.fitting(p.strip).is_some_and(|f| f.kind == FittingKind::PowerStrip && p.outlet < f.ports)
            })
            .copied()
            .collect();
        self.physical.plugs = plugs;
        let wires: Vec<PatchWire> = self.physical.patch_wires.iter().filter(|w| self.pole_ok(w.a) && self.pole_ok(w.b)).copied().collect();
        self.physical.patch_wires = wires;
        let hangs: Vec<Hang> = self
            .physical
            .hangs
            .iter()
            .filter(|h| self.links.iter().any(|l| is_cable(l, h)) && self.peg_count(h.furniture) > h.peg)
            .copied()
            .collect();
        self.physical.hangs = hangs;
        self.reconcile_places();
    }

    /// Repairs the furniture side of a hostile file.
    pub(crate) fn sanitize_furniture(&mut self) {
        let closets = self.closet_ids();
        let buildings = self.building_ids();
        let ph = &mut self.physical;
        ph.furniture.truncate(MAX_FURNITURE);
        let mut seen = BTreeSet::new();
        ph.furniture.retain(|f| closets.contains(&f.place) && seen.insert(f.id));
        for f in &mut ph.furniture {
            f.name = clean_name(&f.name).unwrap_or_else(|| format!("{} {}", f.kind.label(), f.id));
            f.x = finite(f.x);
            f.y = finite(f.y);
            let (lo, hi) = f.kind.size_range();
            f.size = f.size.clamp(lo, hi);
            if f.kind == FurnitureKind::Table {
                if f.width != 0 {
                    f.width = f.width.clamp(TABLE_WIDTH_RANGE.0, TABLE_WIDTH_RANGE.1);
                }
                if f.depth != 0 {
                    f.depth = f.depth.clamp(TABLE_DEPTH_RANGE.0, TABLE_DEPTH_RANGE.1);
                }
            } else {
                f.width = 0;
                f.depth = 0;
            }
            f.back = f.back && f.kind == FurnitureKind::Rack;
        }
        ph.floor_lists.retain(|p| closets.contains(p));
        ph.floor_lists.sort_unstable();
        ph.floor_lists.dedup();
        ph.fittings.truncate(MAX_FITTINGS);
        let mut seen = BTreeSet::new();
        ph.fittings.retain(|f| (closets.contains(&f.place) || (f.kind.is_wall_mount() && buildings.contains(&f.place))) && seen.insert(f.id));
        for f in &mut ph.fittings {
            f.name = clean_name(&f.name).unwrap_or_else(|| format!("{} {}", f.kind.label(), f.id));
            f.x = finite(f.x);
            f.y = finite(f.y);
            let (lo, hi) = f.kind.port_range();
            f.ports = f.ports.clamp(lo, hi);
        }
        // Mounts: a real item and furniture in one closet, in a slot that fits next to those kept.
        let mut kept: Vec<Mount> = Vec::new();
        let mut items = BTreeSet::new();
        for m in std::mem::take(&mut self.physical.mounts) {
            let Some(f) = self.furniture_piece(m.furniture) else { continue };
            if !self.item_exists(m.item) || self.item_place(m.item) != Some(f.place) || items.contains(&m.item) {
                continue;
            }
            let others: Vec<Mount> = kept.iter().filter(|k| k.furniture == m.furniture).copied().collect();
            if check_slot(f, &others, m.slot, m.units).is_ok() && self.check_accepts(f, m.item).is_ok() {
                items.insert(m.item);
                kept.push(m);
            }
        }
        self.physical.mounts = kept;
        // Plugs: one per device, one per outlet.
        let mut kept: Vec<Plug> = Vec::new();
        for p in std::mem::take(&mut self.physical.plugs) {
            let ok = self.device(p.device).is_some()
                && self.fitting(p.strip).is_some_and(|f| f.kind == FittingKind::PowerStrip && p.outlet < f.ports)
                && self.fitting(p.strip).map(|f| f.place) == self.home_of(p.device).map(|h| h.place)
                && !kept.iter().any(|k| k.device == p.device || (k.strip == p.strip && k.outlet == p.outlet));
            if ok {
                kept.push(p);
            }
        }
        self.physical.plugs = kept;
        // Patch wires: real ends in one closet, each used once, a panel on at least one side.
        let mut kept: Vec<PatchWire> = Vec::new();
        for w in std::mem::take(&mut self.physical.patch_wires) {
            let ok = kept.len() < MAX_PATCH_WIRES
                && w.a != w.b
                && !matches!((w.a, w.b), (Pole::Device(_), Pole::Device(_)))
                && self.pole_ok(w.a)
                && self.pole_ok(w.b)
                && (w.is_cord() || self.pole_place(w.a) == self.pole_place(w.b))
                && !(w.is_cord() && self.check_media(w.a, w.b).is_err())
                && !kept.iter().any(|k| k.clashes(&w));
            if ok {
                kept.push(w);
            }
        }
        self.physical.patch_wires = kept;
        // Hung cables: a real cable and peg, one cable to a peg and one peg to a cable.
        let mut kept: Vec<Hang> = Vec::new();
        for h in std::mem::take(&mut self.physical.hangs) {
            let ok = kept.len() < MAX_HANGS
                && self.links.iter().any(|l| is_cable(l, &h))
                && self.peg_count(h.furniture) > h.peg
                && !kept.iter().any(|k| (k.furniture == h.furniture && k.peg == h.peg) || (k.a == h.a && k.b == h.b) || (k.a == h.b && k.b == h.a));
            if ok {
                kept.push(h);
            }
        }
        self.physical.hangs = kept;
    }
}

/// A rack's width, closet units.
pub const RACK_W: f32 = 300.0;
/// A rack unit's height.
pub const RACK_UNIT_H: f32 = 44.0;
/// What an item in a rack is as wide as.
pub const RACK_ITEM_W: f32 = 244.0;
/// A shelf's and a table's width.
pub const SHELF_W: f32 = 800.0;
/// A shelf level's (and a table row's) height.
pub const SHELF_LEVEL_H: f32 = 110.0;
/// What an item on a shelf or a table is as wide as, with its gap.
pub const CELL_W: f32 = 190.0;
/// The room a table keeps around its cells (so a table of the default width is [`SHELF_W`] wide).
pub const TABLE_MARGIN: f32 = 40.0;
/// A row of hooks on a cable pegboard.
pub const PEG_ROW_H: f32 = 70.0;
/// The name bar at the top of a piece of furniture, and the foot below.
pub const FRAME_HEAD: f32 = 36.0;
pub const FRAME_FOOT: f32 = 14.0;
/// An item standing loose on the closet floor.
pub const FLOOR_CELL_W: f32 = 260.0;
pub const FLOOR_CELL_H: f32 = 104.0;
/// An item standing loose on the floor of a closet laid out as a list.
pub const FLOOR_LIST_W: f32 = 520.0;
/// Loose items to a row of the floor.
pub const FLOOR_COLUMNS: usize = 3;

impl Furniture {
    /// Its width and height, closet units.
    pub fn dims(&self) -> (f32, f32) {
        let levels = self.size as f32;
        match self.kind {
            FurnitureKind::Rack | FurnitureKind::ControlPanel => (RACK_W, FRAME_HEAD + levels * RACK_UNIT_H + FRAME_FOOT),
            FurnitureKind::CablePegboard => (SHELF_W, FRAME_HEAD + levels * PEG_ROW_H + FRAME_FOOT),
            FurnitureKind::Shelf | FurnitureKind::InventoryShelf => (SHELF_W, FRAME_HEAD + levels * SHELF_LEVEL_H + FRAME_FOOT),
            FurnitureKind::Table => {
                (TABLE_MARGIN + self.table_cols() as f32 * CELL_W, FRAME_HEAD + self.table_rows() as f32 * SHELF_LEVEL_H + FRAME_FOOT)
            }
        }
    }

    /// The slot at height `y` of the closet (a rack unit, a shelf level, a table's one place);
    /// none above the name bar or below the last unit.
    pub fn slot_at(&self, y: f32) -> Option<u32> {
        let rel = y - self.y - FRAME_HEAD;
        if !rel.is_finite() || rel < 0.0 {
            return None;
        }
        match self.kind {
            FurnitureKind::Rack | FurnitureKind::ControlPanel => Some((rel / RACK_UNIT_H) as u32).filter(|s| *s < self.size),
            FurnitureKind::Shelf | FurnitureKind::InventoryShelf => Some((rel / SHELF_LEVEL_H) as u32).filter(|s| *s < self.size),
            FurnitureKind::CablePegboard => None,
            FurnitureKind::Table => Some(0),
        }
    }
}

/// A rectangle in closet units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Cell {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

/// An item and the cell it is drawn in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub item: Item,
    pub cell: Cell,
    /// The furniture it stands on (none: loose on the floor).
    pub furniture: Option<u32>,
}

/// Where everything in a closet is drawn.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClosetLayout {
    pub frames: Vec<(u32, Cell)>,
    pub items: Vec<Placed>,
}

impl ClosetLayout {
    /// The cell of an item.
    pub fn cell_of(&self, item: Item) -> Option<Cell> {
        self.items.iter().find(|p| p.item == item).map(|p| p.cell)
    }

    /// Everything's bounding box (none: the closet is empty).
    pub fn bounds(&self) -> Option<Cell> {
        let cells = self.frames.iter().map(|f| f.1).chain(self.items.iter().map(|p| p.cell));
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let mut any = false;
        for c in cells {
            any = true;
            x0 = x0.min(c.x);
            y0 = y0.min(c.y);
            x1 = x1.max(c.x + c.w);
            y1 = y1.max(c.y + c.h);
        }
        any.then_some(Cell { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
    }
}

impl Topology {
    /// Where the furniture and the devices of a closet are drawn: items on furniture in the cell
    /// their slot gives, the rest in rows on the floor below it (devices by number, then passive
    /// devices).
    pub fn closet_layout(&self, place: u32) -> ClosetLayout {
        let furn = self.furniture_in(place);
        let frames: Vec<(u32, Cell)> = furn
            .iter()
            .map(|f| {
                let (w, h) = f.dims();
                (f.id, Cell { x: f.x, y: f.y, w, h })
            })
            .collect();
        let mut items = Vec::new();
        for f in &furn {
            let mut count: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
            for m in self.mounts_on(f.id).into_iter().filter(|m| self.item_place(m.item) == Some(place)) {
                let top = f.y + FRAME_HEAD;
                let cell = match f.kind {
                    FurnitureKind::Rack | FurnitureKind::ControlPanel => Cell {
                        x: f.x + (RACK_W - RACK_ITEM_W) / 2.0,
                        y: top + m.slot as f32 * RACK_UNIT_H,
                        w: RACK_ITEM_W,
                        h: m.units as f32 * RACK_UNIT_H,
                    },
                    FurnitureKind::Shelf | FurnitureKind::InventoryShelf => {
                        let n = count.entry(m.slot).or_insert(0);
                        let col = *n;
                        *n += 1;
                        Cell {
                            x: f.x + 20.0 + (col % SHELF_CAPACITY) as f32 * CELL_W,
                            y: top + m.slot as f32 * SHELF_LEVEL_H,
                            w: CELL_W,
                            h: SHELF_LEVEL_H,
                        }
                    }
                    FurnitureKind::Table | FurnitureKind::CablePegboard => {
                        let n = count.entry(0).or_insert(0);
                        let k = *n;
                        *n += 1;
                        let cols = if f.kind == FurnitureKind::Table { f.table_cols() as usize } else { 4 };
                        Cell { x: f.x + 20.0 + (k % cols) as f32 * CELL_W, y: top + (k / cols) as f32 * SHELF_LEVEL_H, w: CELL_W, h: SHELF_LEVEL_H }
                    }
                };
                items.push(Placed { item: m.item, cell, furniture: Some(f.id) });
            }
        }
        let floor_y = frames.iter().map(|f| f.1.y + f.1.h).fold(f32::MIN, f32::max);
        let floor_y = if frames.is_empty() { 0.0 } else { floor_y + 40.0 };
        let mut loose: Vec<Item> = self.homes().iter().filter(|h| h.place == place).map(|h| Item::Device(h.device)).collect();
        loose.sort();
        loose.extend(self.fittings_in(place).iter().map(|f| Item::Fitting(f.id)));
        let loose = loose.into_iter().filter(|i| self.mount_of(*i).is_none());
        // A list is one item to a row, each as wide as three cells of the grid.
        let (columns, cell_w) = if self.floor_is_list(place) { (1, FLOOR_LIST_W) } else { (FLOOR_COLUMNS, FLOOR_CELL_W) };
        for (i, item) in loose.enumerate() {
            let cell = Cell { x: 20.0 + (i % columns) as f32 * cell_w, y: floor_y + (i / columns) as f32 * FLOOR_CELL_H, w: cell_w, h: FLOOR_CELL_H };
            items.push(Placed { item, cell, furniture: None });
        }
        ClosetLayout { frames, items }
    }

    /// Where new furniture goes: to the right of what stands in the closet.
    pub fn furniture_spot(&self, place: u32) -> (f32, f32) {
        let right = self.furniture_in(place).iter().map(|f| f.x + f.dims().0).fold(f32::MIN, f32::max);
        if right.is_finite() && right > f32::MIN { (right + 40.0, 20.0) } else { (20.0, 20.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physical::{DEFAULT_CLOSET, PlaceKind};
    use crate::{DeviceKind, file};

    fn lab() -> (Topology, DeviceId, DeviceId, DeviceId) {
        let mut t = Topology::new();
        let a = t.add_device(DeviceKind::Switch, None, 0.0, 0.0).unwrap();
        let b = t.add_device(DeviceKind::Switch, None, 100.0, 0.0).unwrap();
        let c = t.add_device(DeviceKind::Router, None, 200.0, 0.0).unwrap();
        (t, a, b, c)
    }

    #[test]
    fn a_lab_without_furniture_saves_none() {
        let (t, ..) = lab();
        assert!(t.physical.is_default());
        let s = file::to_json(&t).unwrap();
        assert!(!s.contains("furniture") && !s.contains("mounts") && !s.contains("plugs"));
    }

    #[test]
    fn furniture_stands_in_closets_only_and_has_limits() {
        let (mut t, ..) = lab();
        assert!(t.add_furniture(FurnitureKind::Rack, None, 1, 0.0, 0.0, None).is_err(), "a city");
        assert!(t.add_furniture(FurnitureKind::Rack, None, 99, 0.0, 0.0, None).is_err());
        assert!(t.add_furniture(FurnitureKind::Rack, None, DEFAULT_CLOSET, 0.0, 0.0, Some(3)).is_err());
        assert!(t.add_furniture(FurnitureKind::Rack, None, DEFAULT_CLOSET, 0.0, 0.0, Some(49)).is_err());
        let r = t.add_furniture(FurnitureKind::Rack, None, DEFAULT_CLOSET, 10.0, 20.0, None).unwrap();
        let s = t.add_furniture(FurnitureKind::Shelf, Some(" Top  "), DEFAULT_CLOSET, 300.0, 20.0, Some(2)).unwrap();
        let tb = t.add_furniture(FurnitureKind::Table, None, DEFAULT_CLOSET, 0.0, 400.0, None).unwrap();
        assert_eq!(t.furniture_piece(r).unwrap().name, "Rack0");
        assert_eq!(t.furniture_piece(s).unwrap().name, "Top");
        assert_eq!(t.furniture_piece(tb).unwrap().size, 1);
        assert_eq!(t.furniture_in(DEFAULT_CLOSET).len(), 3);
        assert!(!t.implicit_layout(), "the layout was written down");
        t.rename_furniture(r, "Main rack").unwrap();
        assert!(t.rename_furniture(r, " ").is_err());
        t.move_furniture(r, 5.0, 6.0).unwrap();
        assert_eq!((t.furniture_piece(r).unwrap().x, t.furniture_piece(r).unwrap().y), (5.0, 6.0));
        assert!(t.move_furniture(99, 0.0, 0.0).is_err());
    }

    #[test]
    fn furniture_is_resized_within_its_range_and_never_under_what_stands_on_it() {
        let (mut t, a, b, _) = lab();
        let r = t.add_furniture(FurnitureKind::Rack, None, DEFAULT_CLOSET, 0.0, 0.0, Some(6)).unwrap();
        t.mount_item(Item::Device(a), r, Some(0), None).unwrap();
        t.mount_item(Item::Device(b), r, Some(4), Some(2)).unwrap();
        t.resize_furniture(r, 12).unwrap();
        assert_eq!(t.furniture_piece(r).unwrap().size, 12);
        // A rack cannot drop below the unit its last device ends on, or outside 4 to 48.
        assert!(t.resize_furniture(r, 5).is_err());
        t.resize_furniture(r, 6).unwrap();
        assert!(t.resize_furniture(r, 3).is_err() && t.resize_furniture(r, 49).is_err() && t.resize_furniture(r, 0).is_err());
        assert_eq!(t.furniture_piece(r).unwrap().size, 6, "a refused resize changes nothing");
        // A shelf has 1 to 8 levels; a table has one size; an unknown piece is an error.
        let s = t.add_furniture(FurnitureKind::Shelf, None, DEFAULT_CLOSET, 0.0, 0.0, Some(2)).unwrap();
        t.resize_furniture(s, 8).unwrap();
        assert!(t.resize_furniture(s, 9).is_err());
        let tb = t.add_furniture(FurnitureKind::Table, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        assert!(t.resize_furniture(tb, 2).is_err());
        assert!(t.resize_furniture(99, 4).is_err());
    }

    #[test]
    fn a_rack_stacks_by_units_and_shelves_and_tables_hold_a_few() {
        let (mut t, a, b, c) = lab();
        let r = t.add_furniture(FurnitureKind::Rack, None, DEFAULT_CLOSET, 0.0, 0.0, Some(6)).unwrap();
        let m = t.mount_item(Item::Device(a), r, None, None).unwrap();
        assert_eq!((m.slot, m.units), (0, 1));
        // The next free slot, and no overlap.
        assert_eq!(t.mount_item(Item::Device(b), r, None, Some(2)).unwrap().slot, 1);
        assert!(t.mount_item(Item::Device(c), r, Some(2), None).is_err(), "unit 3 is taken");
        assert!(t.mount_item(Item::Device(c), r, Some(6), None).is_err(), "past the end");
        assert!(t.mount_item(Item::Device(c), r, Some(5), Some(2)).is_err(), "does not fit");
        assert!(t.mount_item(Item::Device(c), r, Some(0), Some(5)).is_err(), "too many units");
        assert_eq!(t.mount_item(Item::Device(c), r, Some(3), None).unwrap().slot, 3);
        // Moving an item frees its units.
        assert_eq!(t.mount_item(Item::Device(a), r, Some(5), None).unwrap().slot, 5);
        assert_eq!(t.mounts_on(r).iter().map(|m| m.slot).collect::<Vec<_>>(), vec![1, 3, 5]);
        assert!(t.unmount_item(Item::Device(a)));
        assert!(!t.unmount_item(Item::Device(a)));
        // A shelf: four to a level.
        let s = t.add_furniture(FurnitureKind::Shelf, None, DEFAULT_CLOSET, 0.0, 0.0, Some(2)).unwrap();
        let mut ids = Vec::new();
        for _ in 0..SHELF_CAPACITY + 1 {
            ids.push(t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap());
        }
        for d in ids.iter().take(SHELF_CAPACITY) {
            t.mount_item(Item::Device(*d), s, Some(0), None).unwrap();
        }
        assert!(t.mount_item(Item::Device(ids[SHELF_CAPACITY]), s, Some(0), None).is_err(), "level 1 is full");
        assert_eq!(t.mount_item(Item::Device(ids[SHELF_CAPACITY]), s, None, None).unwrap().slot, 1);
        assert!(t.mount_item(Item::Device(ids[0]), s, Some(2), None).is_err(), "no level 3");
        // A table: eight in all, whatever the slot.
        let tb = t.add_furniture(FurnitureKind::Table, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        for _ in 0..TABLE_CAPACITY {
            let d = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
            t.mount_item(Item::Device(d), tb, None, None).unwrap();
        }
        let d = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        assert!(t.mount_item(Item::Device(d), tb, None, None).is_err());
        assert!(t.mount_item(Item::Device(d), tb, Some(1), None).is_err());
    }

    #[test]
    fn leaving_a_closet_or_removing_furniture_takes_an_item_off() {
        let (mut t, a, b, _) = lab();
        let c2 = t.add_place(PlaceKind::Closet, None, Some(crate::physical::DEFAULT_BUILDING), 0.0, 0.0).unwrap();
        let r = t.add_furniture(FurnitureKind::Rack, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        t.mount_item(Item::Device(a), r, None, None).unwrap();
        t.mount_item(Item::Device(b), r, None, None).unwrap();
        // Furniture in another closet pulls the device there.
        let r2 = t.add_furniture(FurnitureKind::Rack, None, c2, 0.0, 0.0, None).unwrap();
        t.mount_item(Item::Device(b), r2, None, None).unwrap();
        assert_eq!(t.home_of(b).unwrap().place, c2);
        assert_eq!(t.mount_of(Item::Device(b)).unwrap().furniture, r2);
        // Moving a device to a closet by hand leaves the rack it was on.
        t.set_home(a, c2, 0.0, 0.0).unwrap();
        assert!(t.mount_of(Item::Device(a)).is_none());
        t.mount_item(Item::Device(a), r2, None, None).unwrap();
        // Removing furniture leaves its items in the closet, loose.
        t.remove_furniture(r2).unwrap();
        assert!(t.mount_of(Item::Device(a)).is_none());
        assert_eq!(t.home_of(a).unwrap().place, c2);
        // Removing a closet takes its furniture; removing a device takes its mount.
        t.mount_item(Item::Device(a), r, None, None).unwrap();
        t.remove_device(a).unwrap();
        assert!(t.physical.mounts.iter().all(|m| m.item != Item::Device(a)));
        t.remove_place(DEFAULT_CLOSET).unwrap();
        assert!(t.furniture_piece(r).is_none());
    }

    #[test]
    fn panels_wire_ports_and_a_complete_path_carries_a_cable() {
        let (mut t, a, b, c) = lab();
        let p1 = t.add_fitting(FittingKind::CopperPatchPanel, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        let p2 = t.add_fitting(FittingKind::CopperPatchPanel, None, DEFAULT_CLOSET, 0.0, 0.0, Some(8)).unwrap();
        assert_eq!(t.fitting(p1).unwrap().name, "Copper Patch Panel0");
        assert_eq!(t.fitting(p2).unwrap().ports, 8);
        let pa = Pole::Panel { fitting: p1, port: 2 };
        let pb = Pole::Panel { fitting: p2, port: 7 };
        let da = Pole::Device(End { device: a, port: 0 });
        let db = Pole::Device(End { device: b, port: 0 });
        assert!(t.add_patch_wire(pa, pa).is_err());
        assert!(t.add_patch_wire(da, db).is_err(), "a plain cable");
        assert!(t.add_patch_wire(pa, Pole::Panel { fitting: p2, port: 8 }).is_err(), "no such port");
        assert!(t.add_patch_wire(Pole::Panel { fitting: 99, port: 0 }, pb).is_err());
        assert!(t.add_patch_wire(Pole::Panel { fitting: p1, port: 24 }, pb).is_err());
        assert!(t.add_patch_wire(Pole::Device(End { device: a, port: 99 }), pa).is_err());
        assert!(t.add_patch_wire(da, pa).unwrap().is_empty(), "one drop carries nothing yet");
        assert!(t.add_patch_wire(db, pa).is_err(), "the panel port has its drop");
        assert!(t.add_patch_wire(pa, pb).unwrap().is_empty(), "the cord reaches no second device yet");
        assert!(t.add_patch_wire(pa, Pole::Panel { fitting: p2, port: 1 }).is_err(), "the panel port has its cord");
        assert!(t.links.is_empty());
        let made = t.add_patch_wire(pb, db).unwrap();
        assert_eq!(made, vec![End { device: a, port: 0 }]);
        assert_eq!(t.links.len(), 1);
        assert!(t.link_is_patched(&t.links[0]));
        assert_eq!(t.patch_paths().len(), 1);
        // The ports are cabled now: no second use.
        assert!(t.add_patch_wire(Pole::Panel { fitting: p1, port: 5 }, da).is_err());
        // A device in another closet cannot be wired across.
        let c2 = t.add_place(PlaceKind::Closet, None, Some(crate::physical::DEFAULT_BUILDING), 0.0, 0.0).unwrap();
        t.set_home(c, c2, 0.0, 0.0).unwrap();
        assert!(t.add_patch_wire(Pole::Device(End { device: c, port: 0 }), Pole::Panel { fitting: p1, port: 9 }).is_err());
        // Taking the wires at a port out takes the cable out.
        assert!(t.remove_patch_wire(pb, None));
        assert!(!t.remove_patch_wire(pb, None));
        assert!(t.links.is_empty());
        assert!(t.patch_paths().is_empty());
        assert_eq!(t.physical.patch_wires.len(), 1, "the drop of the first device is left");
        assert!(t.remove_patch_wire(da, Some(pa)));
        assert!(t.physical.patch_wires.is_empty());
    }

    #[test]
    fn removing_a_panel_or_a_device_takes_its_wires() {
        let (mut t, a, b, _) = lab();
        let p1 = t.add_fitting(FittingKind::CopperPatchPanel, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        let w = |port| Pole::Panel { fitting: p1, port };
        let e = |d, port| Pole::Device(End { device: d, port });
        t.add_patch_wire(e(a, 0), w(0)).unwrap();
        t.add_patch_wire(e(b, 0), w(5)).unwrap();
        assert_eq!(t.add_patch_wire(w(0), w(5)).unwrap().len(), 1, "the cord completes the path");
        assert_eq!(t.links.len(), 1);
        t.remove_device(b).unwrap();
        assert!(t.links.is_empty());
        assert_eq!(t.physical.patch_wires.len(), 2, "the drop of the first device and the cord");
        t.remove_fitting(p1).unwrap();
        assert!(t.physical.patch_wires.is_empty());
        assert!(t.physical.fittings.is_empty());
    }

    #[test]
    fn a_cable_a_path_made_can_still_be_unplugged_by_hand() {
        let (mut t, a, b, _) = lab();
        let p1 = t.add_fitting(FittingKind::CopperPatchPanel, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        let w = |port| Pole::Panel { fitting: p1, port };
        let e = |d, port| Pole::Device(End { device: d, port });
        // A cord between two ports of one panel.
        t.add_patch_wire(e(a, 1), w(0)).unwrap();
        t.add_patch_wire(w(5), e(b, 1)).unwrap();
        let made = t.add_patch_wire(w(0), w(5)).unwrap();
        assert_eq!(made.len(), 1);
        assert!(t.links.iter().any(|l| l.a.port == 1 && l.b.port == 1));
        t.disconnect(End { device: a, port: 1 });
        assert_eq!(t.physical.patch_wires.len(), 3, "the wires stay");
        assert!(t.links.is_empty());
    }

    #[test]
    fn a_strip_powers_what_is_plugged_into_it() {
        let (mut t, a, b, _) = lab();
        let s = t.add_fitting(FittingKind::PowerStrip, None, DEFAULT_CLOSET, 0.0, 0.0, Some(2)).unwrap();
        let p = t.add_fitting(FittingKind::CopperPatchPanel, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        assert_eq!(t.fitting(s).unwrap().name, "Power Distribution Device0");
        assert!(t.fitting(s).unwrap().on);
        assert!(t.plug_in(a, p, None).is_err(), "a patch panel has no outlets");
        assert!(t.plug_in(a, 99, None).is_err());
        assert_eq!(t.plug_in(a, s, None).unwrap().outlet, 0);
        assert_eq!(t.plug_in(b, s, None).unwrap().outlet, 1);
        let c = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        assert!(t.plug_in(c, s, None).is_err(), "no free outlet");
        assert!(t.plug_in(c, s, Some(5)).is_err());
        assert!(t.plug_in(c, s, Some(1)).is_err(), "in use");
        // Plugging again moves the plug.
        assert_eq!(t.plug_in(a, s, Some(0)).unwrap().outlet, 0);
        assert_eq!(t.plugs_in(s).len(), 2);
        assert!(!t.cut_by_strip(a));
        assert_eq!(t.set_strip_power(s, false).unwrap(), vec![a, b]);
        assert!(t.cut_by_strip(a) && t.cut_by_strip(b) && !t.cut_by_strip(c));
        assert!(t.set_strip_power(p, false).is_err());
        assert!(t.unplug(a));
        assert!(!t.unplug(a));
        assert!(!t.cut_by_strip(a));
        // A device in another closet cannot plug in, and leaving unplugs.
        let c2 = t.add_place(PlaceKind::Closet, None, Some(crate::physical::DEFAULT_BUILDING), 0.0, 0.0).unwrap();
        t.set_home(b, c2, 0.0, 0.0).unwrap();
        assert!(t.plug_of(b).is_none());
        assert!(t.plug_in(b, s, None).is_err());
    }

    #[test]
    fn furniture_round_trips_and_hostile_files_are_repaired() {
        let (mut t, a, b, _) = lab();
        let r = t.add_furniture(FurnitureKind::Rack, None, DEFAULT_CLOSET, 1.0, 2.0, Some(8)).unwrap();
        let s = t.add_fitting(FittingKind::PowerStrip, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        let p = t.add_fitting(FittingKind::CopperPatchPanel, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        t.mount_item(Item::Device(a), r, None, Some(2)).unwrap();
        t.mount_item(Item::Fitting(p), r, None, None).unwrap();
        t.plug_in(a, s, None).unwrap();
        t.add_patch_wire(Pole::Device(End { device: b, port: 3 }), Pole::Panel { fitting: p, port: 0 }).unwrap();
        let text = file::to_json(&t).unwrap();
        assert_eq!(file::from_json(&text).unwrap(), t);
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let ph = &mut v["topology"]["physical"];
        ph["furniture"] = serde_json::json!([
            {"id": 1, "place": 3, "kind": "rack", "name": "", "x": 1e30, "size": 4000},
            {"id": 1, "place": 3, "kind": "shelf", "name": "dup"},
            {"id": 2, "place": 77, "kind": "table", "name": "nowhere"},
            {"id": 3, "place": 1, "kind": "table", "name": "in a city"},
            {"id": 4, "place": 3, "kind": "shelf", "size": 0}
        ]);
        ph["fittings"] = serde_json::json!([
            {"id": 1, "place": 3, "kind": "copper-patch-panel", "name": "P", "ports": 9000},
            {"id": 2, "place": 3, "kind": "power-strip", "name": "S", "ports": 0, "on": false},
            {"id": 3, "place": 77, "kind": "power-strip", "name": "Z"}
        ]);
        let item = |d: u32| serde_json::json!({"device": d});
        ph["mounts"] = serde_json::json!([
            {"item": item(a.0), "furniture": 1, "slot": 0, "units": 2},
            {"item": item(b.0), "furniture": 1, "slot": 1, "units": 1},
            {"item": item(b.0), "furniture": 1, "slot": 40},
            {"item": item(9999), "furniture": 1, "slot": 5},
            {"item": {"fitting": 1}, "furniture": 4, "slot": 0},
            {"item": {"fitting": 1}, "furniture": 4, "slot": 7},
            {"item": {"fitting": 2}, "furniture": 9, "slot": 0}
        ]);
        ph["plugs"] = serde_json::json!([
            {"device": a.0, "strip": 2, "outlet": 0},
            {"device": a.0, "strip": 2, "outlet": 1},
            {"device": b.0, "strip": 2, "outlet": 0},
            {"device": b.0, "strip": 1, "outlet": 0},
            {"device": b.0, "strip": 2, "outlet": 99}
        ]);
        let pole = |f: u32, port: u32| serde_json::json!({"panel": {"fitting": f, "port": port}});
        let dev = |d: u32, port: u32| serde_json::json!({"device": {"device": d, "port": port}});
        ph["patch_wires"] = serde_json::json!([
            {"a": pole(1, 0), "b": dev(b.0, 3)},
            {"a": pole(1, 0), "b": dev(a.0, 3)},
            {"a": dev(a.0, 0), "b": dev(b.0, 0)},
            {"a": pole(1, 5), "b": pole(1, 5)},
            {"a": pole(1, 99), "b": dev(a.0, 1)},
            {"a": pole(2, 0), "b": dev(a.0, 1)}
        ]);
        let r = file::from_json(&v.to_string()).unwrap();
        let ph = &r.physical;
        assert_eq!(ph.furniture.iter().map(|f| f.id).collect::<Vec<_>>(), vec![1, 4]);
        assert!(ph.furniture.iter().all(|f| !f.name.is_empty() && f.x.abs() <= 1e6));
        assert_eq!(ph.furniture[0].size, 48);
        assert_eq!(ph.furniture[1].size, 1);
        assert_eq!(ph.fittings.iter().map(|f| (f.id, f.ports)).collect::<Vec<_>>(), vec![(1, 48), (2, 2)]);
        assert!(!ph.fittings[1].on);
        // The first mount holds units 1-2 of the rack; the overlap at slot 1 is dropped.
        assert_eq!(ph.mounts.len(), 3, "{:?}", ph.mounts);
        assert!(ph.mounts.iter().any(|m| m.item == Item::Device(a) && m.units == 2));
        assert!(ph.mounts.iter().any(|m| m.item == Item::Device(b) && m.slot == 40));
        assert_eq!(ph.plugs.len(), 1);
        assert_eq!(ph.plugs[0], Plug { device: a, strip: 2, outlet: 0 });
        assert_eq!(ph.patch_wires.len(), 1);
        // Oversized lists are capped.
        let many: Vec<serde_json::Value> =
            (1..=MAX_FURNITURE as u32 + 50).map(|i| serde_json::json!({"id": i, "place": 3, "kind": "table", "name": format!("T{i}")})).collect();
        v["topology"]["physical"]["furniture"] = serde_json::Value::Array(many);
        assert_eq!(file::from_json(&v.to_string()).unwrap().physical.furniture.len(), MAX_FURNITURE);
    }

    #[test]
    fn the_layout_puts_items_in_their_cells_and_the_rest_on_the_floor() {
        let (mut t, a, b, c) = lab();
        // Without furniture every device stands on the floor from the top, three to a row.
        let l = t.closet_layout(DEFAULT_CLOSET);
        assert_eq!(l.items.len(), 3);
        assert!(l.frames.is_empty() && l.items.iter().all(|i| i.furniture.is_none()));
        assert_eq!(l.cell_of(Item::Device(a)).unwrap().y, 0.0);
        assert!(l.cell_of(Item::Device(c)).unwrap().x > l.cell_of(Item::Device(b)).unwrap().x);
        let r = t.add_furniture(FurnitureKind::Rack, None, DEFAULT_CLOSET, 20.0, 20.0, Some(6)).unwrap();
        assert_eq!(t.furniture_spot(DEFAULT_CLOSET), (20.0 + RACK_W + 40.0, 20.0));
        let s = t.add_furniture(FurnitureKind::Shelf, None, DEFAULT_CLOSET, 400.0, 20.0, Some(2)).unwrap();
        t.mount_item(Item::Device(a), r, Some(1), Some(2)).unwrap();
        t.mount_item(Item::Device(b), s, Some(1), None).unwrap();
        t.mount_item(Item::Device(c), s, Some(1), None).unwrap();
        let l = t.closet_layout(DEFAULT_CLOSET);
        assert_eq!(l.frames.len(), 2);
        let ca = l.cell_of(Item::Device(a)).unwrap();
        assert_eq!((ca.y, ca.h), (20.0 + FRAME_HEAD + RACK_UNIT_H, 2.0 * RACK_UNIT_H));
        let (cb, cc) = (l.cell_of(Item::Device(b)).unwrap(), l.cell_of(Item::Device(c)).unwrap());
        assert_eq!(cb.y, cc.y);
        assert_eq!(cc.x - cb.x, CELL_W);
        // The rack is 6 units tall: slot_at reads units, and nothing above the name bar.
        let f = t.furniture_piece(r).unwrap();
        assert_eq!(f.slot_at(20.0 + FRAME_HEAD + 1.0), Some(0));
        assert_eq!(f.slot_at(20.0 + FRAME_HEAD + RACK_UNIT_H * 5.5), Some(5));
        assert_eq!(f.slot_at(20.0 + FRAME_HEAD + RACK_UNIT_H * 6.5), None);
        assert_eq!(f.slot_at(20.0 + 5.0), None);
        // A loose passive device and the floor under the tallest piece.
        let p = t.add_fitting(FittingKind::CopperPatchPanel, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        let l = t.closet_layout(DEFAULT_CLOSET);
        let cp = l.cell_of(Item::Fitting(p)).unwrap();
        assert!(l.items.iter().find(|i| i.item == Item::Fitting(p)).unwrap().furniture.is_none());
        let tall = l.frames.iter().map(|f| f.1.y + f.1.h).fold(f32::MIN, f32::max);
        assert_eq!(cp.y, tall + 40.0);
        let bounds = l.bounds().unwrap();
        assert!(bounds.w > 0.0 && bounds.h > 0.0);
        assert!(ClosetLayout::default().bounds().is_none());
    }

    #[test]
    fn wall_mounts_and_fiber_panels_carry_a_link_of_their_medium() {
        let (mut t, a, b, _) = lab();
        let wall = t.add_fitting(FittingKind::CopperWallMount, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        let wall2 = t.add_fitting(FittingKind::CopperWallMount, None, DEFAULT_CLOSET, 0.0, 0.0, Some(1)).unwrap();
        let fiber = t.add_fitting(FittingKind::FiberWallMount, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        assert_eq!(t.fitting(wall).unwrap().name, "Copper Wall Mount0");
        assert_eq!(t.fitting(wall).unwrap().ports, 2);
        assert!(t.add_fitting(FittingKind::CopperWallMount, None, DEFAULT_CLOSET, 0.0, 0.0, Some(5)).is_err());
        let w = |f, port| Pole::Panel { fitting: f, port };
        let e = |d, port| Pole::Device(End { device: d, port });
        // A copper wall outlet takes a copper port, not a fiber one, and joins only copper.
        assert!(t.add_patch_wire(e(a, 0), w(fiber, 0)).is_err());
        assert!(t.add_patch_wire(w(wall, 0), w(fiber, 1)).is_err());
        // Two wall mounts, a cord between them: a link through the building's cabling.
        t.add_patch_wire(e(a, 0), w(wall, 0)).unwrap();
        t.add_patch_wire(e(b, 0), w(wall2, 0)).unwrap();
        let made = t.add_patch_wire(w(wall, 0), w(wall2, 0)).unwrap();
        assert_eq!(made.len(), 1);
        assert_eq!(t.links.len(), 1);
        assert!(!FittingKind::PowerStrip.is_cabling());
        assert_eq!(FittingKind::from_id("Fiber Wall Mount"), Some(FittingKind::FiberWallMount));
        assert_eq!(FittingKind::from_id("pdu"), Some(FittingKind::PowerStrip));
        assert_eq!(FittingKind::from_id("nope"), None);
    }

    #[test]
    fn containers_hold_furniture_and_the_new_kinds_follow_their_rules() {
        use crate::physical::{DEFAULT_BUILDING, DEFAULT_CITY};
        let (mut t, a, b, _) = lab();
        // A container sits in a city or a building, never on the Intercity map or in a closet.
        assert!(t.add_place(PlaceKind::Container, None, None, 0.0, 0.0).is_err());
        assert!(t.add_place(PlaceKind::Container, None, Some(DEFAULT_CLOSET), 0.0, 0.0).is_err());
        let in_city = t.add_place(PlaceKind::Container, None, Some(DEFAULT_CITY), 0.0, 0.0).unwrap();
        let in_building = t.add_place(PlaceKind::Container, Some("Store"), Some(DEFAULT_BUILDING), 0.0, 0.0).unwrap();
        assert_eq!(t.place(in_city).unwrap().name, "Container 1");
        assert_eq!(t.city_of(in_city).map(|c| c.id), Some(DEFAULT_CITY));
        assert_eq!(t.city_of(in_building).map(|c| c.id), Some(DEFAULT_CITY));
        let rack = t.add_furniture(FurnitureKind::Rack, None, in_building, 0.0, 0.0, None).unwrap();
        t.mount_item(Item::Device(a), rack, None, None).unwrap();
        assert_eq!(t.home_of(a).unwrap().place, in_building);
        // Inventory shelf: stored devices are switched off, and stay off on a rack.
        let inv = t.add_furniture(FurnitureKind::InventoryShelf, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        assert_eq!(t.furniture_piece(inv).unwrap().name, "Inventory Shelf0");
        assert!(!t.device(b).unwrap().powered_off);
        t.mount_item(Item::Device(b), inv, None, None).unwrap();
        assert!(t.device(b).unwrap().powered_off);
        let table = t.add_furniture(FurnitureKind::Table, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        t.mount_item(Item::Device(b), table, None, None).unwrap();
        assert!(t.device(b).unwrap().powered_off, "it keeps the power state it had");
        // Cable pegboard: no devices, cables on pegs.
        let peg = t.add_furniture(FurnitureKind::CablePegboard, None, DEFAULT_CLOSET, 0.0, 0.0, Some(2)).unwrap();
        assert!(t.mount_item(Item::Device(b), peg, None, None).is_err());
        let c = t.add_device(DeviceKind::Switch, None, 0.0, 0.0).unwrap();
        let d = t.add_device(DeviceKind::Switch, None, 0.0, 0.0).unwrap();
        t.set_home(c, DEFAULT_CLOSET, 0.0, 0.0).unwrap();
        t.set_home(d, DEFAULT_CLOSET, 0.0, 0.0).unwrap();
        let e = End { device: c, port: 0 };
        assert!(t.hang_cable(peg, e, None).is_err(), "no cable yet");
        t.connect(e, End { device: d, port: 0 }).unwrap();
        assert_eq!(t.peg_count(peg), 2 * PEGS_PER_ROW);
        assert_eq!(t.hang_cable(peg, e, None).unwrap().peg, 0);
        assert_eq!(t.hang_cable(peg, End { device: d, port: 0 }, Some(3)).unwrap().peg, 3, "the same cable moves");
        assert_eq!(t.hangs_on(peg).len(), 1);
        assert!(t.hang_cable(peg, e, Some(2 * PEGS_PER_ROW)).is_err());
        assert!(t.unhang_cable(e));
        assert!(!t.unhang_cable(e));
        t.hang_cable(peg, e, None).unwrap();
        t.disconnect(e);
        assert!(t.hangs_on(peg).is_empty(), "a cable that is gone leaves its peg");
        // Control panel: industrial devices only.
        let icp = t.add_furniture(FurnitureKind::ControlPanel, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        assert!(t.mount_item(Item::Device(c), icp, None, None).is_err(), "a plain switch is not industrial");
        let plc = t.add_model(crate::models::find("PLC").unwrap(), None, 0.0, 0.0).unwrap();
        t.set_home(plc, DEFAULT_CLOSET, 0.0, 0.0).unwrap();
        t.mount_item(Item::Device(plc), icp, None, Some(2)).unwrap();
        assert!(t.add_furniture(FurnitureKind::ControlPanel, None, DEFAULT_CLOSET, 0.0, 0.0, Some(5)).is_err());
        assert!(t.add_furniture(FurnitureKind::CablePegboard, None, DEFAULT_CLOSET, 0.0, 0.0, Some(9)).is_err());
        // Save, load, and a hostile file.
        let text = file::to_json(&t).unwrap();
        assert_eq!(file::from_json(&text).unwrap(), t);
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v["topology"]["physical"]["hangs"] =
            serde_json::json!([{"furniture": 999, "peg": 0, "a": {"device": 1, "port": 0}, "b": {"device": 2, "port": 0}}]);
        v["topology"]["physical"]["working"] = serde_json::json!(4000);
        assert!(file::from_json(&v.to_string()).unwrap().physical.hangs.is_empty());
        assert_eq!(file::from_json(&v.to_string()).unwrap().physical.working, None);
        v["topology"]["physical"]["furniture"][0]["kind"] = serde_json::json!("sofa");
        assert!(file::from_json(&v.to_string()).is_err(), "an unknown kind is refused, not guessed");
    }

    #[test]
    fn the_working_closet_defaults_to_the_first_and_can_be_set() {
        use crate::physical::DEFAULT_BUILDING;
        let (mut t, ..) = lab();
        assert_eq!(t.working_closet(), Some(DEFAULT_CLOSET));
        let c2 = t.add_place(PlaceKind::Closet, None, Some(DEFAULT_BUILDING), 0.0, 0.0).unwrap();
        assert_eq!(t.working_closet(), Some(DEFAULT_CLOSET));
        assert!(t.set_working_closet(DEFAULT_BUILDING).is_err());
        assert!(t.set_working_closet(999).is_err());
        t.set_working_closet(c2).unwrap();
        assert_eq!(t.working_closet(), Some(c2));
        assert_eq!(file::from_json(&file::to_json(&t).unwrap()).unwrap().working_closet(), Some(c2));
        t.remove_place(c2).unwrap();
        assert_eq!(t.working_closet(), Some(DEFAULT_CLOSET), "it falls back when the marked closet is gone");
    }

    #[test]
    fn a_table_is_resized_in_cells_and_never_under_what_stands_on_it() {
        let (mut t, a, b, c) = lab();
        let tb = t.add_furniture(FurnitureKind::Table, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        let f = t.furniture_piece(tb).unwrap();
        assert_eq!((f.table_cols(), f.table_rows(), f.table_capacity()), (TABLE_COLS, TABLE_ROWS, TABLE_CAPACITY));
        assert_eq!(f.dims().0, SHELF_W, "a table as made is as wide as it always was");
        t.resize_table(tb, Some(6), Some(3)).unwrap();
        let f = t.furniture_piece(tb).unwrap();
        assert_eq!((f.width, f.depth, f.table_capacity()), (6, 3, 18));
        assert_eq!(f.dims(), (TABLE_MARGIN + 6.0 * CELL_W, FRAME_HEAD + 3.0 * SHELF_LEVEL_H + FRAME_FOOT));
        // Only the width: the depth stays.
        t.resize_table(tb, Some(2), None).unwrap();
        assert_eq!(t.furniture_piece(tb).unwrap().table_rows(), 3);
        for d in [a, b, c] {
            t.mount_item(Item::Device(d), tb, None, None).unwrap();
        }
        let cells = t.closet_layout(DEFAULT_CLOSET);
        let at = |d| cells.cell_of(Item::Device(d)).unwrap();
        assert!(at(b).x > at(a).x && at(c).y > at(a).y, "two cells to a row: the third item starts the next row");
        // Under what stands on it: refused, and nothing changes.
        assert!(t.resize_table(tb, Some(2), Some(1)).is_err());
        assert_eq!(t.furniture_piece(tb).unwrap().table_capacity(), 6);
        t.resize_table(tb, Some(3), Some(1)).unwrap();
        let strip = t.add_fitting(FittingKind::PowerStrip, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        assert!(t.mount_item(Item::Fitting(strip), tb, None, None).is_err(), "a full table takes no more");
        // Out of range, nothing to change, not a table, no such table.
        assert!(t.resize_table(tb, Some(1), None).is_err() && t.resize_table(tb, Some(9), None).is_err());
        assert!(t.resize_table(tb, None, Some(0)).is_err() && t.resize_table(tb, None, Some(7)).is_err());
        assert!(t.resize_table(tb, None, None).is_err());
        let r = t.add_furniture(FurnitureKind::Rack, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        assert!(t.resize_table(r, Some(3), None).is_err() && t.resize_table(99, Some(3), None).is_err());
        // Back to the default size: nothing is written.
        t.unmount_item(Item::Device(a));
        t.unmount_item(Item::Device(b));
        t.unmount_item(Item::Device(c));
        t.resize_table(tb, Some(TABLE_COLS), Some(TABLE_ROWS)).unwrap();
        let f = t.furniture_piece(tb).unwrap();
        assert_eq!((f.width, f.depth), (0, 0));
        assert!(!serde_json::to_string(t.furniture_piece(tb).unwrap()).unwrap().contains("width"));
    }

    #[test]
    fn only_a_rack_turns_around_and_the_side_is_saved() {
        let (mut t, ..) = lab();
        let r = t.add_furniture(FurnitureKind::Rack, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        let s = t.add_furniture(FurnitureKind::Shelf, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        assert!(!t.furniture_piece(r).unwrap().back);
        assert!(t.turn_furniture(r, None).unwrap(), "it flips");
        assert!(!t.turn_furniture(r, None).unwrap());
        assert!(t.turn_furniture(r, Some(true)).unwrap() && t.turn_furniture(r, Some(true)).unwrap(), "naming the side is idempotent");
        assert!(t.turn_furniture(s, None).is_err() && t.turn_furniture(99, None).is_err());
        let again = file::from_json(&file::to_json(&t).unwrap()).unwrap();
        assert!(again.furniture_piece(r).unwrap().back);
        // A hostile file cannot give a shelf a back or a rack a table's size.
        let mut v: serde_json::Value = serde_json::from_str(&file::to_json(&t).unwrap()).unwrap();
        v["topology"]["physical"]["furniture"][1]["back"] = serde_json::json!(true);
        v["topology"]["physical"]["furniture"][1]["width"] = serde_json::json!(5);
        v["topology"]["physical"]["furniture"][0]["depth"] = serde_json::json!(9);
        let r2 = file::from_json(&v.to_string()).unwrap();
        assert!(r2.physical.furniture.iter().all(|f| f.width == 0 && f.depth == 0));
        assert!(!r2.furniture_piece(s).unwrap().back);
        // A table's size in a hostile file is clamped.
        let tb = t.add_furniture(FurnitureKind::Table, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&file::to_json(&t).unwrap()).unwrap();
        v["topology"]["physical"]["furniture"][2]["width"] = serde_json::json!(4000);
        v["topology"]["physical"]["furniture"][2]["depth"] = serde_json::json!(1);
        let f = file::from_json(&v.to_string()).unwrap().furniture_piece(tb).cloned().unwrap();
        assert_eq!((f.width, f.depth), (TABLE_WIDTH_RANGE.1, 1));
    }

    #[test]
    fn the_loose_items_of_a_closet_are_laid_out_as_a_grid_or_a_list() {
        use crate::physical::DEFAULT_BUILDING;
        let (mut t, a, b, c) = lab();
        assert!(!t.floor_is_list(DEFAULT_CLOSET));
        let grid = t.closet_layout(DEFAULT_CLOSET);
        let (ga, gb) = (grid.cell_of(Item::Device(a)).unwrap(), grid.cell_of(Item::Device(b)).unwrap());
        assert_eq!(ga.y, gb.y, "a grid puts neighbours side by side");
        t.set_floor_list(DEFAULT_CLOSET, true).unwrap();
        assert!(t.floor_is_list(DEFAULT_CLOSET));
        let list = t.closet_layout(DEFAULT_CLOSET);
        let cells: Vec<Cell> = [a, b, c].iter().map(|d| list.cell_of(Item::Device(*d)).unwrap()).collect();
        assert!(cells.windows(2).all(|w| w[0].x == w[1].x && w[1].y > w[0].y), "a list is one item to a row: {cells:?}");
        assert!(cells[0].w > ga.w);
        let again = file::from_json(&file::to_json(&t).unwrap()).unwrap();
        assert!(again.floor_is_list(DEFAULT_CLOSET), "the choice is saved with the lab");
        t.set_floor_list(DEFAULT_CLOSET, false).unwrap();
        assert!(t.physical.floor_lists.is_empty());
        assert!(t.set_floor_list(DEFAULT_BUILDING, true).is_err() && t.set_floor_list(999, true).is_err());
        // A place that goes takes its setting along; a hostile file cannot list a city or repeat itself.
        let c2 = t.add_place(PlaceKind::Closet, None, Some(DEFAULT_BUILDING), 0.0, 0.0).unwrap();
        t.set_floor_list(c2, true).unwrap();
        t.remove_place(c2).unwrap();
        assert!(t.physical.floor_lists.is_empty());
        let mut v: serde_json::Value = serde_json::from_str(&file::to_json(&t).unwrap()).unwrap();
        v["topology"]["physical"]["floor_lists"] = serde_json::json!([3, 3, 1, 77]);
        assert_eq!(file::from_json(&v.to_string()).unwrap().physical.floor_lists, vec![3]);
    }

    /// Two closets, a switch in each and a patch panel in each: the drops are wired, the backbone is not.
    fn two_closets() -> (Topology, DeviceId, DeviceId, u32, u32, u32) {
        use crate::physical::DEFAULT_BUILDING;
        let (mut t, a, b, _) = lab();
        let c2 = t.add_place(PlaceKind::Closet, Some("Annex"), Some(DEFAULT_BUILDING), 0.0, 0.0).unwrap();
        t.set_home(b, c2, 10.0, 10.0).unwrap();
        let p1 = t.add_fitting(FittingKind::CopperPatchPanel, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        let p2 = t.add_fitting(FittingKind::CopperPatchPanel, None, c2, 0.0, 0.0, None).unwrap();
        t.add_patch_wire(Pole::Device(End { device: a, port: 0 }), Pole::Panel { fitting: p1, port: 0 }).unwrap();
        t.add_patch_wire(Pole::Device(End { device: b, port: 0 }), Pole::Panel { fitting: p2, port: 4 }).unwrap();
        (t, a, b, p1, p2, c2)
    }

    #[test]
    fn a_backbone_cables_two_devices_standing_in_two_closets() {
        let (mut t, a, b, p1, p2, c2) = two_closets();
        let (ea, eb) = (End { device: a, port: 0 }, End { device: b, port: 0 });
        assert!(t.link_at(ea).is_none(), "the drops alone carry nothing");
        let (pa, pb) = (Pole::Panel { fitting: p1, port: 0 }, Pole::Panel { fitting: p2, port: 4 });
        // A patch cord does not leave its closet; a backbone does.
        let e = t.add_patch_wire(pa, pb).unwrap_err().to_string();
        assert!(e.contains("backbone"), "{e}");
        let made = t.add_backbone(pa, pb).unwrap();
        assert_eq!(made.len(), 1, "the path is complete: the devices are cabled");
        let l = *t.link_at(ea).unwrap();
        assert!(l.a == eb || l.b == eb);
        assert!(t.link_is_patched(&l));
        assert_eq!(t.backbones().len(), 1);
        let (near, far) = t.backbone_of_link(&l).unwrap();
        assert!((near, far) == (pa, pb) || (near, far) == (pb, pa));
        assert_eq!(t.backbone_at(pb), Some((pb, pa)));
        assert_eq!(t.pole_place(pb), Some(c2));
        assert_eq!(t.run_length(a, b).meters, t.cable_length(&l).meters);
        // Removing the backbone unplugs the cable and keeps the drops.
        assert!(t.remove_backbone(pa, None));
        assert!(t.link_at(ea).is_none() && t.link_at(eb).is_none());
        assert_eq!(t.physical.patch_wires.len(), 2);
        assert!(!t.remove_backbone(pa, None), "nothing left to remove");
        // The drops can go in last: the path completes with the second one.
        let (mut t, a, b, p1, p2, _) = two_closets();
        t.remove_patch_wire(Pole::Device(End { device: b, port: 0 }), None);
        t.add_backbone(Pole::Panel { fitting: p1, port: 3 }, Pole::Panel { fitting: p2, port: 4 }).unwrap();
        t.remove_patch_wire(Pole::Device(End { device: a, port: 0 }), None);
        t.add_patch_wire(Pole::Device(End { device: a, port: 1 }), Pole::Panel { fitting: p1, port: 3 }).unwrap();
        assert!(t.link_at(End { device: a, port: 1 }).is_none());
        let made = t.add_patch_wire(Pole::Device(End { device: b, port: 2 }), Pole::Panel { fitting: p2, port: 4 }).unwrap();
        assert_eq!(made.len(), 1, "the second drop closes the path");
        assert!(t.link_at(End { device: b, port: 2 }).is_some());
    }

    #[test]
    fn a_backbone_follows_the_rules_of_the_cabling() {
        let (mut t, _, _, p1, p2, c2) = two_closets();
        let port = |f: u32, n: u32| Pole::Panel { fitting: f, port: n };
        // Two different places, two panel ports of one medium, each port used once.
        let q1 = t.add_fitting(FittingKind::CopperPatchPanel, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        assert!(t.add_backbone(port(p1, 1), port(q1, 1)).is_err(), "one closet is not a backbone");
        assert!(t.add_backbone(port(p1, 1), port(p1, 1)).is_err());
        assert!(t.add_backbone(port(p1, 1), Pole::Device(End { device: DeviceId(1), port: 3 })).is_err());
        let fiber = t.add_fitting(FittingKind::FiberPatchPanel, None, c2, 0.0, 0.0, None).unwrap();
        assert!(t.add_backbone(port(p1, 1), port(fiber, 0)).is_err(), "copper does not join fiber");
        assert!(t.add_backbone(port(p1, 99), port(p2, 1)).is_err());
        t.add_backbone(port(p1, 1), port(p2, 1)).unwrap();
        assert!(t.add_backbone(port(p1, 1), port(p2, 2)).is_err() && t.add_backbone(port(p1, 2), port(p2, 1)).is_err(), "a port takes one backbone");
        t.add_patch_wire(port(p1, 5), port(q1, 5)).unwrap();
        assert!(t.add_backbone(port(p1, 5), port(p2, 5)).is_err(), "a port takes one cord, or one backbone");
        assert_eq!(t.first_free_cord_port(p1), Some(0), "port 1 and 5 are taken, 0 has only a drop");
        assert_eq!(t.first_free_cord_port(fiber), Some(0));
        assert_eq!(t.first_free_cord_port(999), None);
        // A wall mount in a room (a container in the building) is a backbone end like a panel.
        let room = t.add_place(PlaceKind::Container, Some("Room 2"), Some(crate::physical::DEFAULT_BUILDING), 0.0, 0.0).unwrap();
        let wall = t.add_fitting(FittingKind::CopperWallMount, None, room, 0.0, 0.0, None).unwrap();
        t.add_backbone(port(wall, 0), port(p1, 7)).unwrap();
        assert_eq!(t.backbones().len(), 2);
        // A power strip has no ports to wire.
        let strip = t.add_fitting(FittingKind::PowerStrip, None, room, 0.0, 0.0, None).unwrap();
        assert!(t.add_backbone(port(strip, 0), port(p1, 9)).is_err());
        // Taking a panel away takes its backbone with it.
        t.remove_fitting(p2).unwrap();
        assert_eq!(t.backbones().len(), 1);
        t.remove_place(room).unwrap();
        assert!(t.backbones().is_empty(), "so does taking the place away");
    }

    #[test]
    fn a_backbone_is_saved_and_a_hostile_one_is_repaired() {
        let (mut t, a, b, p1, p2, _) = two_closets();
        t.add_backbone(Pole::Panel { fitting: p1, port: 0 }, Pole::Panel { fitting: p2, port: 4 }).unwrap();
        let text = file::to_json(&t).unwrap();
        let back = file::from_json(&text).unwrap();
        assert_eq!(back, t);
        assert_eq!(back.backbones().len(), 1);
        assert!(back.link_at(End { device: a, port: 0 }).is_some_and(|l| l.a.device == b || l.b.device == b));
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let wires = v["topology"]["physical"]["patch_wires"].as_array().unwrap().clone();
        let pole = |f: u32, port: u32| serde_json::json!({"panel": {"fitting": f, "port": port}});
        let mut hostile = wires.clone();
        hostile.push(serde_json::json!({"a": pole(p1, 6), "b": pole(999, 0)}));
        hostile.push(serde_json::json!({"a": pole(p1, 0), "b": pole(p2, 9)}));
        hostile.push(serde_json::json!({"a": pole(p2, 11), "b": pole(p1, 12)}));
        hostile.push(serde_json::json!({"a": pole(p2, 11), "b": pole(p1, 13)}));
        v["topology"]["physical"]["patch_wires"] = serde_json::Value::Array(hostile);
        let r = file::from_json(&v.to_string()).unwrap();
        assert_eq!(r.backbones().len(), 2, "the one to a missing panel and the second use of a port are dropped: {:?}", r.physical.patch_wires);
    }

    #[test]
    fn a_wall_mount_stands_in_a_room_of_a_building_and_a_backbone_reaches_the_closet() {
        use crate::physical::DEFAULT_BUILDING;
        let mut t = Topology::new();
        let pc = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        let sw = t.add_device(DeviceKind::Switch, None, 100.0, 0.0).unwrap();
        // The PC stands in a room of the building (a host may), the switch in the closet.
        t.set_home(pc, DEFAULT_BUILDING, 5.0, 5.0).unwrap();
        let wall = t.add_fitting(FittingKind::CopperWallMount, None, DEFAULT_BUILDING, 0.0, 0.0, Some(2)).unwrap();
        assert!(
            t.add_fitting(FittingKind::CopperPatchPanel, None, DEFAULT_BUILDING, 0.0, 0.0, None).is_err(),
            "only a wall mount stands on a room's wall"
        );
        assert!(t.add_fitting(FittingKind::PowerStrip, None, DEFAULT_BUILDING, 0.0, 0.0, None).is_err());
        assert!(t.add_fitting(FittingKind::CopperWallMount, None, crate::physical::DEFAULT_CITY, 0.0, 0.0, None).is_err(), "not in a city");
        let panel = t.add_fitting(FittingKind::CopperPatchPanel, None, DEFAULT_CLOSET, 0.0, 0.0, None).unwrap();
        t.add_patch_wire(Pole::Device(End { device: pc, port: 0 }), Pole::Panel { fitting: wall, port: 0 }).unwrap();
        t.add_patch_wire(Pole::Device(End { device: sw, port: 0 }), Pole::Panel { fitting: panel, port: 3 }).unwrap();
        assert!(
            t.add_patch_wire(Pole::Device(End { device: sw, port: 1 }), Pole::Panel { fitting: wall, port: 1 }).is_err(),
            "a drop stays in its place"
        );
        let made = t.add_backbone(Pole::Panel { fitting: wall, port: 0 }, Pole::Panel { fitting: panel, port: 3 }).unwrap();
        assert_eq!(made.len(), 1);
        assert!(t.link_at(End { device: pc, port: 0 }).is_some(), "the PC in the room reaches the switch in the closet");
        let again = file::from_json(&file::to_json(&t).unwrap()).unwrap();
        assert_eq!(again, t, "the wall mount in the building survives a save");
        // A building that goes takes its wall mounts and their backbones along.
        let mut t = t;
        t.remove_place(DEFAULT_BUILDING).unwrap();
        assert!(t.physical.fittings.is_empty() && t.backbones().is_empty());
    }
}
