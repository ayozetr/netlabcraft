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

/// What a piece of furniture is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FurnitureKind {
    Rack,
    Shelf,
    Table,
}

impl FurnitureKind {
    pub const ALL: [FurnitureKind; 3] = [FurnitureKind::Rack, FurnitureKind::Shelf, FurnitureKind::Table];

    pub fn id(self) -> &'static str {
        match self {
            FurnitureKind::Rack => "rack",
            FurnitureKind::Shelf => "shelf",
            FurnitureKind::Table => "table",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FurnitureKind::Rack => "Rack",
            FurnitureKind::Shelf => "Shelf",
            FurnitureKind::Table => "Table",
        }
    }

    pub fn from_id(s: &str) -> Option<FurnitureKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "rack" => Some(FurnitureKind::Rack),
            "shelf" => Some(FurnitureKind::Shelf),
            "table" => Some(FurnitureKind::Table),
            _ => None,
        }
    }

    /// Rack units, shelf levels (a table has one place to stand things).
    pub fn default_size(self) -> u32 {
        match self {
            FurnitureKind::Rack => 12,
            FurnitureKind::Shelf => 4,
            FurnitureKind::Table => 1,
        }
    }

    /// The fewest and most units (levels) it can have.
    pub fn size_range(self) -> (u32, u32) {
        match self {
            FurnitureKind::Rack => (4, 48),
            FurnitureKind::Shelf => (1, 8),
            FurnitureKind::Table => (1, 1),
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
    pub name: String,
    /// Where it stands in the closet (closet units).
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    /// Rack units or shelf levels.
    #[serde(default)]
    pub size: u32,
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
        match (self.is_fiber(), medium) {
            (_, Medium::Combo) => true,
            (true, Medium::Fiber) | (false, Medium::Copper) => true,
            _ => false,
        }
    }
}

fn yes() -> bool {
    true
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

    fn is_cord(&self) -> bool {
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
        FurnitureKind::Rack => {
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
        FurnitureKind::Shelf => {
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
            if others.len() >= TABLE_CAPACITY {
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

    fn closet_ids(&self) -> BTreeSet<u32> {
        self.places().iter().filter(|p| p.kind == PlaceKind::Closet).map(|p| p.id).collect()
    }

    fn closet_of(&self, place: u32) -> Result<()> {
        let p = self.place(place).ok_or_else(|| ModelError::Invalid(format!("no place {place}")))?;
        if p.kind != PlaceKind::Closet {
            return Err(ModelError::Invalid("furniture and passive devices stand in a wiring closet".into()));
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
            return Err(ModelError::Invalid(format!(
                "a {} has {lo} to {hi} {}",
                kind.label().to_lowercase(),
                if kind == FurnitureKind::Rack { "units" } else { "levels" }
            )));
        }
        self.materialize_places();
        let id = self.physical.furniture.iter().map(|f| f.id).max().unwrap_or(0).saturating_add(1);
        let n = self.physical.furniture.iter().filter(|f| f.kind == kind).count();
        let name = name.and_then(clean_name).unwrap_or_else(|| format!("{}{n}", kind.label()));
        self.physical.furniture.push(Furniture { id, place, kind, name, x: finite(x), y: finite(y), size });
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
        Ok(f)
    }

    /// The first slot where an item of `units` units fits on a piece of furniture.
    pub fn first_free_slot(&self, item: Item, furniture: u32, units: u32) -> Option<u32> {
        let f = self.furniture_piece(furniture)?;
        let others: Vec<Mount> = self.physical.mounts.iter().filter(|m| m.furniture == furniture && m.item != item).copied().collect();
        let slots = if f.kind == FurnitureKind::Table { 1 } else { f.size };
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
        Ok(m)
    }

    /// Takes an item off its furniture; it stays in the closet. Whether it was on furniture.
    pub fn unmount_item(&mut self, item: Item) -> bool {
        let before = self.physical.mounts.len();
        self.physical.mounts.retain(|m| m.item != item);
        self.physical.mounts.len() != before
    }

    /// Adds a patch panel or a power distribution device to a closet; returns its id.
    pub fn add_fitting(&mut self, kind: FittingKind, name: Option<&str>, place: u32, x: f32, y: f32, ports: Option<u32>) -> Result<u32> {
        self.closet_of(place)?;
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
        self.physical.fittings.push(Fitting { id, place, kind, name, x: finite(x), y: finite(y), ports, on: true });
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
        if self.pole_place(a) != self.pole_place(b) {
            return Err(ModelError::Invalid("both ends of a patch wire stand in the same wiring closet".into()));
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

    /// Switches a power distribution device; returns the devices plugged into it.
    pub fn set_strip_power(&mut self, strip: u32, on: bool) -> Result<Vec<DeviceId>> {
        let f = self.fitting_mut(strip)?;
        if f.kind != FittingKind::PowerStrip {
            return Err(ModelError::Invalid(format!("{} has no power switch", f.name)));
        }
        f.on = on;
        Ok(self.plugs_in(strip).iter().map(|p| p.device).collect())
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
            .filter(|w| self.pole_place(w.a).is_none() || self.pole_place(w.a) != self.pole_place(w.b))
            .copied()
            .collect();
        for w in moved {
            self.remove_patch_wire(w.a, Some(w.b));
        }
    }

    /// Forgets what refers to places, devices or passive devices that are gone.
    pub(crate) fn prune_furniture(&mut self) {
        let closets = self.closet_ids();
        self.physical.furniture.retain(|f| closets.contains(&f.place));
        self.physical.fittings.retain(|f| closets.contains(&f.place));
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
        self.reconcile_places();
    }

    /// Repairs the furniture side of a hostile file.
    pub(crate) fn sanitize_furniture(&mut self) {
        let closets = self.closet_ids();
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
        }
        ph.fittings.truncate(MAX_FITTINGS);
        let mut seen = BTreeSet::new();
        ph.fittings.retain(|f| closets.contains(&f.place) && seen.insert(f.id));
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
            if check_slot(f, &others, m.slot, m.units).is_ok() {
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
                && self.pole_place(w.a) == self.pole_place(w.b)
                && !kept.iter().any(|k| k.clashes(&w));
            if ok {
                kept.push(w);
            }
        }
        self.physical.patch_wires = kept;
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
/// The name bar at the top of a piece of furniture, and the foot below.
pub const FRAME_HEAD: f32 = 36.0;
pub const FRAME_FOOT: f32 = 14.0;
/// An item standing loose on the closet floor.
pub const FLOOR_CELL_W: f32 = 260.0;
pub const FLOOR_CELL_H: f32 = 104.0;
/// Loose items to a row of the floor.
pub const FLOOR_COLUMNS: usize = 3;

impl Furniture {
    /// Its width and height, closet units.
    pub fn dims(&self) -> (f32, f32) {
        let levels = self.size as f32;
        match self.kind {
            FurnitureKind::Rack => (RACK_W, FRAME_HEAD + levels * RACK_UNIT_H + FRAME_FOOT),
            FurnitureKind::Shelf => (SHELF_W, FRAME_HEAD + levels * SHELF_LEVEL_H + FRAME_FOOT),
            FurnitureKind::Table => (SHELF_W, FRAME_HEAD + 2.0 * SHELF_LEVEL_H + FRAME_FOOT),
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
            FurnitureKind::Rack => Some((rel / RACK_UNIT_H) as u32).filter(|s| *s < self.size),
            FurnitureKind::Shelf => Some((rel / SHELF_LEVEL_H) as u32).filter(|s| *s < self.size),
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
                    FurnitureKind::Rack => Cell {
                        x: f.x + (RACK_W - RACK_ITEM_W) / 2.0,
                        y: top + m.slot as f32 * RACK_UNIT_H,
                        w: RACK_ITEM_W,
                        h: m.units as f32 * RACK_UNIT_H,
                    },
                    FurnitureKind::Shelf => {
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
                    FurnitureKind::Table => {
                        let n = count.entry(0).or_insert(0);
                        let k = *n;
                        *n += 1;
                        Cell { x: f.x + 20.0 + (k % 4) as f32 * CELL_W, y: top + (k / 4) as f32 * SHELF_LEVEL_H, w: CELL_W, h: SHELF_LEVEL_H }
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
        for (i, item) in loose.enumerate() {
            let cell = Cell {
                x: 20.0 + (i % FLOOR_COLUMNS) as f32 * FLOOR_CELL_W,
                y: floor_y + (i / FLOOR_COLUMNS) as f32 * FLOOR_CELL_H,
                w: FLOOR_CELL_W,
                h: FLOOR_CELL_H,
            };
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
    use crate::physical::DEFAULT_CLOSET;
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
}
