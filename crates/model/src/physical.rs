//! The physical view: where the equipment stands. Places nest Intercity (the top map) → cities
//! → buildings → wiring closets; every device lives in a closet (a host may also stand in a
//! building's rooms). A cable's length follows from the places of its two ends, unless one is
//! set by hand, and a copper cable longer than [`COPPER_MAX_M`] carries nothing.
//!
//! A lab that never touched the physical view has no places stored: it is shown with an
//! implicit layout (one city, one building, one closet holding every device), which is written
//! down the first time a place is changed.

use std::borrow::Cow;
use std::collections::BTreeSet;

use netlabcraft_core::DeviceId;
use serde::{Deserialize, Serialize};

use crate::furniture::{Fitting, Furniture, Hang, Mount, PatchWire, Plug};
use crate::{End, Link, MAX_DEVICES, MAX_NAME, ModelError, Result, Topology, finite};

/// Most places in a lab.
pub const MAX_PLACES: usize = 500;
/// Most hand-set cable lengths kept.
pub const MAX_LENGTHS: usize = 10_000;
/// Longest cable length accepted, in metres (20 000 km).
pub const MAX_CABLE_M: u32 = 20_000_000;
/// Longest copper Ethernet run that still carries a signal, in metres.
pub const COPPER_MAX_M: u32 = 100;
/// Two ends in the same closet (or the same room).
pub const SAME_PLACE_M: u32 = 3;
/// Two ends in different closets or rooms of one building.
pub const SAME_BUILDING_M: u32 = 30;
/// A city map unit, in metres (buildings).
pub const CITY_UNIT_M: f32 = 1.0;
/// An Intercity map unit, in metres (cities).
pub const INTERCITY_UNIT_M: f32 = 100.0;

/// The ids of the implicit layout (also used when it is written down).
pub const DEFAULT_CITY: u32 = 1;
pub const DEFAULT_BUILDING: u32 = 2;
pub const DEFAULT_CLOSET: u32 = 3;

/// What a place is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaceKind {
    City,
    Building,
    #[default]
    Closet,
    /// A generic container: a plain room-like place in a city or a building that holds devices
    /// and furniture like a wiring closet.
    Container,
}

impl PlaceKind {
    pub const ALL: [PlaceKind; 4] = [PlaceKind::City, PlaceKind::Building, PlaceKind::Closet, PlaceKind::Container];

    pub fn id(self) -> &'static str {
        match self {
            PlaceKind::City => "city",
            PlaceKind::Building => "building",
            PlaceKind::Closet => "closet",
            PlaceKind::Container => "container",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PlaceKind::City => "City",
            PlaceKind::Building => "Building",
            PlaceKind::Closet => "Wiring closet",
            PlaceKind::Container => "Container",
        }
    }

    pub fn from_id(s: &str) -> Option<PlaceKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "city" => Some(PlaceKind::City),
            "building" => Some(PlaceKind::Building),
            "closet" | "wiring-closet" | "rack" => Some(PlaceKind::Closet),
            "container" | "generic-container" => Some(PlaceKind::Container),
            _ => None,
        }
    }

    /// The kinds of place this one can sit in (none: it is on the Intercity map).
    pub fn parent_kinds(self) -> &'static [PlaceKind] {
        match self {
            PlaceKind::City => &[],
            PlaceKind::Building => &[PlaceKind::City],
            PlaceKind::Closet => &[PlaceKind::Building],
            PlaceKind::Container => &[PlaceKind::City, PlaceKind::Building],
        }
    }

    /// The kinds of place that can sit in this one.
    pub fn child_kinds(self) -> &'static [PlaceKind] {
        match self {
            PlaceKind::City => &[PlaceKind::Building, PlaceKind::Container],
            PlaceKind::Building => &[PlaceKind::Closet, PlaceKind::Container],
            PlaceKind::Closet | PlaceKind::Container => &[],
        }
    }

    /// The kind of place the plain "new place" button makes here (a building in a city, a
    /// closet in a building).
    pub fn child_kind(self) -> Option<PlaceKind> {
        match self {
            PlaceKind::City => Some(PlaceKind::Building),
            PlaceKind::Building => Some(PlaceKind::Closet),
            PlaceKind::Closet | PlaceKind::Container => None,
        }
    }

    /// Whether devices and furniture stand in it: a wiring closet or a container.
    pub fn holds_things(self) -> bool {
        matches!(self, PlaceKind::Closet | PlaceKind::Container)
    }
}

/// A city, building or wiring closet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub id: u32,
    #[serde(default)]
    pub kind: PlaceKind,
    pub name: String,
    /// Where its icon sits on its parent's map.
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    /// The place it is in (none: a city on the Intercity map).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u32>,
}

/// Where a device stands, and where inside that place it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Home {
    pub device: DeviceId,
    pub place: u32,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
}

/// A cable length set by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CableLength {
    pub a: End,
    pub b: End,
    pub meters: u32,
}

/// The splices set by hand on a fiber cable (each one costs a little light).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CableSplices {
    pub a: End,
    pub b: End,
    pub count: u32,
}

/// The physical side of a lab.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Physical {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub places: Vec<Place>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub homes: Vec<Home>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lengths: Vec<CableLength>,
    /// Splices on fiber cables (a cable with none is not listed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub splices: Vec<CableSplices>,
    /// Racks, shelves and tables standing in wiring closets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub furniture: Vec<Furniture>,
    /// Passive devices: patch panels and power distribution devices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fittings: Vec<Fitting>,
    /// What stands on which furniture, and where on it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<Mount>,
    /// Patch wires: panel port to panel port, panel port to a device port.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patch_wires: Vec<PatchWire>,
    /// Devices plugged into a power distribution device's outlets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugs: Vec<Plug>,
    /// Cables hung on the pegs of cable pegboards.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hangs: Vec<Hang>,
    /// The closet the user works in (the Home button goes there); none: the first closet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working: Option<u32>,
    /// What each place shows: its background, its icon and its size in metres.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenes: Vec<crate::backdrop::Scene>,
    /// The lab ignores cable lengths: a copper run over [`COPPER_MAX_M`] (or another medium's
    /// limit) still carries frames. Not set, lengths count, as they always did.
    #[serde(default, skip_serializing_if = "is_false")]
    pub ignore_lengths: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Physical {
    pub fn is_default(&self) -> bool {
        self.places.is_empty()
            && self.homes.is_empty()
            && self.lengths.is_empty()
            && self.splices.is_empty()
            && self.furniture.is_empty()
            && self.fittings.is_empty()
            && self.mounts.is_empty()
            && self.patch_wires.is_empty()
            && self.plugs.is_empty()
            && self.hangs.is_empty()
            && self.working.is_none()
            && self.scenes.is_empty()
            && !self.ignore_lengths
    }
}

/// The layout shown for a lab without places.
pub fn default_layout() -> Vec<Place> {
    vec![
        Place { id: DEFAULT_CITY, kind: PlaceKind::City, name: "Home City".into(), x: 300.0, y: 220.0, parent: None },
        Place { id: DEFAULT_BUILDING, kind: PlaceKind::Building, name: "Main Building".into(), x: 300.0, y: 220.0, parent: Some(DEFAULT_CITY) },
        Place { id: DEFAULT_CLOSET, kind: PlaceKind::Closet, name: "Main Wiring Closet".into(), x: 300.0, y: 220.0, parent: Some(DEFAULT_BUILDING) },
    ]
}

/// How long a cable is and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Length {
    pub meters: u32,
    /// Set by hand (else derived from the places of its ends).
    pub manual: bool,
}

pub(crate) fn clean_name(name: &str) -> Option<String> {
    let n: String = name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    let n = n.trim().to_string();
    (!n.is_empty()).then_some(n)
}

fn same_cable(l: &Link, c: &CableLength) -> bool {
    same_ends(l, c.a, c.b)
}

fn same_ends(l: &Link, a: End, b: End) -> bool {
    (l.a == a && l.b == b) || (l.a == b && l.b == a)
}

/// Most splices on one cable.
pub const MAX_SPLICES: u32 = 50;
/// Most cables with splices kept.
pub const MAX_SPLICED: usize = 10_000;
/// Loss of one mated connector pair, in dB (the ceiling structured-cabling standards allow).
pub const CONNECTOR_LOSS_DB: f32 = 0.75;
/// Loss of one fusion splice, in dB.
pub const SPLICE_LOSS_DB: f32 = 0.3;
/// Attenuation of multi-mode fiber at 850 nm, in dB a kilometre.
pub const MULTI_MODE_DB_KM: f32 = 3.0;
/// Attenuation of single-mode fiber at 1310 nm, in dB a kilometre.
pub const SINGLE_MODE_DB_KM: f32 = 0.35;
/// What a short-reach (850 nm) transceiver tolerates between its ends, in dB.
pub const MULTI_MODE_BUDGET_DB: f32 = 3.56;
/// What a long-reach (1310 nm) transceiver tolerates between its ends, in dB.
pub const SINGLE_MODE_BUDGET_DB: f32 = 6.0;
/// A link with less than this left over works but is flagged as degraded, in dB.
pub const BUDGET_MARGIN_DB: f32 = 1.0;
/// Connectors on a cable run: one at each end.
pub const END_CONNECTORS: u32 = 2;

/// How a fiber link stands against its transceivers' power budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetState {
    /// Margin to spare.
    Ok,
    /// It carries traffic, with under [`BUDGET_MARGIN_DB`] to spare.
    Degraded,
    /// The loss exceeds the budget: the link is down.
    Over,
}

/// The light a fiber link loses (distance, connectors, splices) against what its transceivers tolerate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FiberBudget {
    pub meters: u32,
    pub connectors: u32,
    pub splices: u32,
    pub loss_db: f32,
    pub budget_db: f32,
}

impl FiberBudget {
    /// What is left of the budget, in dB (negative when it is exceeded).
    pub fn margin_db(&self) -> f32 {
        self.budget_db - self.loss_db
    }

    pub fn state(&self) -> BudgetState {
        let margin = self.margin_db();
        if margin < 0.0 {
            BudgetState::Over
        } else if margin < BUDGET_MARGIN_DB {
            BudgetState::Degraded
        } else {
            BudgetState::Ok
        }
    }
}

impl Topology {
    /// The places: those stored, or the implicit layout.
    pub fn places(&self) -> Cow<'_, [Place]> {
        if self.physical.places.is_empty() { Cow::Owned(default_layout()) } else { Cow::Borrowed(&self.physical.places) }
    }

    /// Whether the lab uses the implicit layout (no places stored yet).
    pub fn implicit_layout(&self) -> bool {
        self.physical.places.is_empty()
    }

    pub fn place(&self, id: u32) -> Option<Place> {
        self.places().iter().find(|p| p.id == id).cloned()
    }

    /// The closet the Home button goes to: the one marked as working, else the first closet.
    pub fn working_closet(&self) -> Option<u32> {
        let places = self.places();
        self.physical
            .working
            .filter(|w| places.iter().any(|p| p.id == *w && p.kind == PlaceKind::Closet))
            .or_else(|| places.iter().filter(|p| p.kind == PlaceKind::Closet).map(|p| p.id).min())
    }

    /// Marks a closet as the working one.
    pub fn set_working_closet(&mut self, id: u32) -> Result<()> {
        let p = self.place(id).ok_or_else(|| ModelError::Invalid(format!("no place {id}")))?;
        if p.kind != PlaceKind::Closet {
            return Err(ModelError::Invalid("the working closet is a wiring closet".into()));
        }
        self.materialize_places();
        self.physical.working = Some(id);
        Ok(())
    }

    /// Writes the implicit layout down, so it can be changed.
    pub fn materialize_places(&mut self) {
        if self.physical.places.is_empty() {
            self.physical.places = default_layout();
        }
    }

    /// Where devices without a home of their own stand: the first closet, else the first
    /// building, else the first city.
    pub fn default_place(&self) -> u32 {
        let places = self.places();
        [PlaceKind::Closet, PlaceKind::Container, PlaceKind::Building, PlaceKind::City]
            .iter()
            .find_map(|k| places.iter().find(|p| p.kind == *k).map(|p| p.id))
            .unwrap_or(DEFAULT_CLOSET)
    }

    /// Where a device stands: its place and its position there.
    pub fn home_of(&self, dev: DeviceId) -> Option<Home> {
        let d = self.device(dev)?;
        let places = self.places();
        if let Some(h) = self.physical.homes.iter().find(|h| h.device == dev && places.iter().any(|p| p.id == h.place)) {
            return Some(*h);
        }
        Some(Home { device: dev, place: self.default_place(), x: d.x, y: d.y })
    }

    /// Every device's home, at once.
    pub fn homes(&self) -> Vec<Home> {
        let places = self.places();
        let ids: BTreeSet<u32> = places.iter().map(|p| p.id).collect();
        let fallback = self.default_place();
        self.devices
            .iter()
            .map(|d| {
                self.physical.homes.iter().find(|h| h.device == d.id && ids.contains(&h.place)).copied().unwrap_or(Home {
                    device: d.id,
                    place: fallback,
                    x: d.x,
                    y: d.y,
                })
            })
            .collect()
    }

    /// From a city down to `place` (for a path bar); empty for the Intercity map.
    pub fn place_path(&self, place: Option<u32>) -> Vec<Place> {
        let places = self.places();
        let mut out: Vec<Place> = Vec::new();
        let mut at = place;
        while let Some(id) = at {
            if out.len() > 3 || out.iter().any(|p| p.id == id) {
                break;
            }
            let Some(p) = places.iter().find(|p| p.id == id) else { break };
            out.push(p.clone());
            at = p.parent;
        }
        out.reverse();
        out
    }

    /// The places drawn on the map of `parent` (none: the Intercity map).
    pub fn places_in(&self, parent: Option<u32>) -> Vec<Place> {
        self.places().iter().filter(|p| p.parent == parent).cloned().collect()
    }

    /// The place and every place inside it.
    pub fn place_subtree(&self, id: u32) -> BTreeSet<u32> {
        let places = self.places();
        let mut out = BTreeSet::from([id]);
        // Three levels at most; a few passes settle it.
        for _ in 0..4 {
            for p in places.iter() {
                if p.parent.is_some_and(|q| out.contains(&q)) {
                    out.insert(p.id);
                }
            }
        }
        out
    }

    /// The building a place belongs to (itself for a building).
    pub fn building_of(&self, place: u32) -> Option<Place> {
        let p = self.place(place)?;
        match p.kind {
            PlaceKind::Building => Some(p),
            PlaceKind::Closet => p.parent.and_then(|b| self.place(b)),
            PlaceKind::Container => p.parent.and_then(|b| self.place(b)).filter(|b| b.kind == PlaceKind::Building),
            PlaceKind::City => None,
        }
    }

    /// The city a place belongs to (itself for a city).
    pub fn city_of(&self, place: u32) -> Option<Place> {
        let p = self.place(place)?;
        match p.kind {
            PlaceKind::City => Some(p),
            PlaceKind::Container if p.parent.and_then(|c| self.place(c)).is_some_and(|c| c.kind == PlaceKind::City) => {
                p.parent.and_then(|c| self.place(c))
            }
            _ => self.building_of(place).and_then(|b| b.parent).and_then(|c| self.place(c)),
        }
    }

    fn next_place_id(&self) -> u32 {
        self.physical.places.iter().map(|p| p.id).max().unwrap_or(0).saturating_add(1)
    }

    /// Adds a city (no parent), a building (in a city) or a closet (in a building); returns its id.
    pub fn add_place(&mut self, kind: PlaceKind, name: Option<&str>, parent: Option<u32>, x: f32, y: f32) -> Result<u32> {
        self.materialize_places();
        if self.physical.places.len() >= MAX_PLACES {
            return Err(ModelError::Invalid(format!("at most {MAX_PLACES} places")));
        }
        let kinds = kind.parent_kinds();
        let parent = if kinds.is_empty() {
            None
        } else {
            let wanted = kinds.iter().map(|k| k.label().to_lowercase()).collect::<Vec<_>>().join(" or ");
            let id = parent.ok_or_else(|| ModelError::Invalid(format!("say which {wanted} the {} goes in", kind.label().to_lowercase())))?;
            let p = self.place(id).ok_or_else(|| ModelError::Invalid(format!("no place {id}")))?;
            if !kinds.contains(&p.kind) {
                return Err(ModelError::Invalid(format!(
                    "a {} goes in a {wanted}, not in a {}",
                    kind.label().to_lowercase(),
                    p.kind.label().to_lowercase()
                )));
            }
            Some(id)
        };
        let id = self.next_place_id();
        let name = name.and_then(clean_name).unwrap_or_else(|| {
            let n = self.physical.places.iter().filter(|p| p.kind == kind).count() + 1;
            match kind {
                PlaceKind::City => format!("City {n}"),
                PlaceKind::Building => format!("Building {n}"),
                PlaceKind::Closet => format!("Closet {n}"),
                PlaceKind::Container => format!("Container {n}"),
            }
        });
        self.physical.places.push(Place { id, kind, name, x: finite(x), y: finite(y), parent });
        Ok(id)
    }

    fn stored_place_mut(&mut self, id: u32) -> Result<&mut Place> {
        self.materialize_places();
        self.physical.places.iter_mut().find(|p| p.id == id).ok_or_else(|| ModelError::Invalid(format!("no place {id}")))
    }

    pub fn rename_place(&mut self, id: u32, name: &str) -> Result<()> {
        let name = clean_name(name).ok_or_else(|| ModelError::Invalid("a place needs a name".into()))?;
        self.stored_place_mut(id)?.name = name;
        Ok(())
    }

    /// Moves a place's icon on its parent's map.
    pub fn move_place(&mut self, id: u32, x: f32, y: f32) -> Result<()> {
        let p = self.stored_place_mut(id)?;
        p.x = finite(x);
        p.y = finite(y);
        Ok(())
    }

    /// Removes a place and the places inside it. The devices there move up to the place it was
    /// in when that is a building, else to the default closet. The last city stays.
    pub fn remove_place(&mut self, id: u32) -> Result<Place> {
        self.materialize_places();
        let p = self.place(id).ok_or_else(|| ModelError::Invalid(format!("no place {id}")))?;
        if p.kind == PlaceKind::City && self.physical.places.iter().filter(|q| q.kind == PlaceKind::City).count() <= 1 {
            return Err(ModelError::Invalid("a lab keeps at least one city".into()));
        }
        let gone = self.place_subtree(id);
        let up = p.parent.and_then(|q| self.place(q)).filter(|q| q.kind == PlaceKind::Building).map(|q| q.id);
        if self.physical.working.is_some_and(|w| gone.contains(&w)) {
            self.physical.working = None;
        }
        self.physical.places.retain(|q| !gone.contains(&q.id));
        match up {
            Some(b) => {
                for h in &mut self.physical.homes {
                    if gone.contains(&h.place) {
                        h.place = b;
                    }
                }
            }
            None => self.physical.homes.retain(|h| !gone.contains(&h.place)),
        }
        self.prune_furniture();
        self.prune_environments();
        self.prune_scenes();
        Ok(p)
    }

    /// Puts a device in a place, at a position there. Network devices stand in closets; hosts
    /// in a closet or a building.
    pub fn set_home(&mut self, dev: DeviceId, place: u32, x: f32, y: f32) -> Result<()> {
        let d = self.device(dev).ok_or_else(|| ModelError::NoDevice(dev.to_string()))?;
        let host = d.kind.is_host();
        let name = d.name.clone();
        let p = self.place(place).ok_or_else(|| ModelError::Invalid(format!("no place {place}")))?;
        match p.kind {
            PlaceKind::Closet | PlaceKind::Container => {}
            PlaceKind::Building if host => {}
            PlaceKind::Building => {
                return Err(ModelError::Invalid(format!("{name} is network equipment: it goes in a wiring closet, not in a building's rooms")));
            }
            PlaceKind::City => return Err(ModelError::Invalid(format!("{name} needs a building or a wiring closet, not a city"))),
        }
        self.materialize_places();
        let home = Home { device: dev, place, x: finite(x), y: finite(y) };
        match self.physical.homes.iter_mut().find(|h| h.device == dev) {
            Some(h) => *h = home,
            None => self.physical.homes.push(home),
        }
        // Furniture and power strips are tied to a closet: a device that left one is off them.
        self.reconcile_places();
        Ok(())
    }

    /// A cable's length: set by hand, else from where its two ends stand.
    pub fn cable_length(&self, l: &Link) -> Length {
        // Most labs never touch the physical view: every cable runs inside one closet.
        if self.physical.is_default() {
            return Length { meters: SAME_PLACE_M, manual: false };
        }
        if let Some(c) = self.physical.lengths.iter().find(|c| same_cable(l, c)) {
            return Length { meters: c.meters, manual: true };
        }
        self.run_length(l.a.device, l.b.device)
    }

    /// The length of a run between two devices that has no hand-set length, from where they
    /// stand: what a console, USB or octal cable measures.
    pub fn run_length(&self, a: DeviceId, b: DeviceId) -> Length {
        if self.physical.is_default() {
            return Length { meters: SAME_PLACE_M, manual: false };
        }
        let (Some(ha), Some(hb)) = (self.home_of(a), self.home_of(b)) else {
            return Length { meters: SAME_PLACE_M, manual: false };
        };
        // Two devices in one place whose map has a scale of its own (its size was set, perhaps
        // with a plan of the floor as its background): the distance on the map is the cable.
        if ha.place == hb.place && self.map_scale_is_custom(ha.place) {
            let d = ((ha.x - hb.x).powi(2) + (ha.y - hb.y).powi(2)).sqrt() * self.map_scale(ha.place);
            return Length { meters: to_meters(d).max(1), manual: false };
        }
        Length { meters: self.distance(ha.place, hb.place), manual: false }
    }

    /// Why a console, USB or octal cable between two devices carries nothing because it is longer
    /// than its standard allows (`None`: it reaches, or the lab ignores lengths).
    pub fn run_problem(&self, kind: crate::cable::Cable, a: DeviceId, b: DeviceId) -> Option<String> {
        if self.physical.ignore_lengths {
            return None;
        }
        let reach = kind.reach()?;
        let len = self.run_length(a, b);
        (len.meters > reach.meters).then(|| format!("cable too long: {} m {}, max {} m", len.meters, reach.what, reach.meters))
    }

    /// The cable run between two places, in metres.
    pub fn distance(&self, a: u32, b: u32) -> u32 {
        if a == b {
            return SAME_PLACE_M;
        }
        let (ba, bb) = (self.building_of(a), self.building_of(b));
        if let (Some(x), Some(y)) = (&ba, &bb) {
            if x.id == y.id {
                return SAME_BUILDING_M;
            }
            if x.parent.is_some() && x.parent == y.parent {
                let scale = x.parent.map_or(CITY_UNIT_M, |c| self.map_scale(c));
                let d = ((x.x - y.x).powi(2) + (x.y - y.y).powi(2)).sqrt() * scale;
                return to_meters(d).saturating_add(SAME_BUILDING_M);
            }
        }
        let (ca, cb) = (self.city_of(a), self.city_of(b));
        let d = match (ca, cb) {
            (Some(x), Some(y)) => ((x.x - y.x).powi(2) + (x.y - y.y).powi(2)).sqrt() * self.map_scale(crate::backdrop::INTERCITY),
            _ => 0.0,
        };
        to_meters(d).saturating_add(SAME_BUILDING_M.saturating_mul(2))
    }

    /// Sets (or with `None` clears) the hand-set length of the cable at `end`.
    pub fn set_cable_length(&mut self, end: End, meters: Option<u32>) -> Result<Length> {
        let l = *self.link_at(end).ok_or_else(|| ModelError::Invalid(format!("{} has no cable", self.end_label(end))))?;
        self.physical.lengths.retain(|c| !same_cable(&l, c));
        if let Some(m) = meters {
            if m == 0 || m > MAX_CABLE_M {
                return Err(ModelError::Invalid(format!("a cable is 1 to {MAX_CABLE_M} metres long")));
            }
            if self.physical.lengths.len() >= MAX_LENGTHS {
                return Err(ModelError::Invalid(format!("at most {MAX_LENGTHS} cable lengths")));
            }
            self.physical.lengths.push(CableLength { a: l.a, b: l.b, meters: m });
        }
        Ok(self.cable_length(&l))
    }

    /// The splices on the cable at a link.
    pub fn splices_on(&self, l: &Link) -> u32 {
        self.physical.splices.iter().find(|c| same_ends(l, c.a, c.b)).map_or(0, |c| c.count)
    }

    /// Sets the splices on the cable at `end` (0 clears them); returns the count.
    pub fn set_splices(&mut self, end: End, count: u32) -> Result<u32> {
        let l = *self.link_at(end).ok_or_else(|| ModelError::Invalid(format!("{} has no cable", self.end_label(end))))?;
        if !self.resolved_cable(&l).is_fiber() {
            return Err(ModelError::Invalid("only a fiber cable has splices".into()));
        }
        if count > MAX_SPLICES {
            return Err(ModelError::Invalid(format!("a cable has 0 to {MAX_SPLICES} splices")));
        }
        self.physical.splices.retain(|c| !same_ends(&l, c.a, c.b));
        if count > 0 {
            if self.physical.splices.len() >= MAX_SPLICED {
                return Err(ModelError::Invalid(format!("at most {MAX_SPLICED} cables with splices")));
            }
            self.physical.splices.push(CableSplices { a: l.a, b: l.b, count });
        }
        Ok(count)
    }

    /// The loss budget of a fiber cable: the run's length, its two end connectors and its
    /// splices against what the transceiver at its first end tolerates. `None` for a cable
    /// that is not fiber.
    pub fn fiber_budget(&self, l: &Link) -> Option<FiberBudget> {
        let kind = self.resolved_cable(l);
        if !kind.is_fiber() {
            return None;
        }
        let meters = self.cable_length(l).meters;
        let (connectors, splices) = (END_CONNECTORS, self.splices_on(l));
        let per_km = if kind == crate::cable::Cable::FiberSingle { SINGLE_MODE_DB_KM } else { MULTI_MODE_DB_KM };
        let loss_db = meters as f32 / 1000.0 * per_km + connectors as f32 * CONNECTOR_LOSS_DB + splices as f32 * SPLICE_LOSS_DB;
        let budget_db = if self.single_mode_at(l.a) { SINGLE_MODE_BUDGET_DB } else { MULTI_MODE_BUDGET_DB };
        Some(FiberBudget { meters, connectors, splices, loss_db, budget_db })
    }

    /// Drops what refers to devices or cables that are gone.
    pub(crate) fn prune_physical(&mut self) {
        let devices: BTreeSet<DeviceId> = self.devices.iter().map(|d| d.id).collect();
        self.physical.homes.retain(|h| devices.contains(&h.device));
        let links = &self.links;
        self.physical.lengths.retain(|c| links.iter().any(|l| same_cable(l, c)));
        self.physical.splices.retain(|c| links.iter().any(|l| same_ends(l, c.a, c.b)));
        self.prune_furniture();
    }

    /// Repairs the physical side of a hostile file: repeated ids, wrong nesting, unknown
    /// devices, places and cables, oversized lists.
    pub(crate) fn sanitize_physical(&mut self) {
        let ph = &mut self.physical;
        ph.places.truncate(MAX_PLACES);
        let mut ids = BTreeSet::new();
        ph.places.retain(|p| ids.insert(p.id));
        for p in &mut ph.places {
            p.name = clean_name(&p.name).unwrap_or_else(|| format!("{} {}", p.kind.label(), p.id));
            p.x = finite(p.x);
            p.y = finite(p.y);
        }
        // Cities, then buildings, then closets and containers: each must sit in a place of a
        // kind that can hold it.
        for kind in PlaceKind::ALL {
            let fits: Vec<(u32, PlaceKind)> = ph.places.iter().map(|p| (p.id, p.kind)).collect();
            let wanted = kind.parent_kinds();
            let first_parent = wanted.iter().find_map(|pk| fits.iter().find(|(_, k)| k == pk).map(|(i, _)| *i));
            let mut drop = BTreeSet::new();
            for p in ph.places.iter_mut().filter(|p| p.kind == kind) {
                if wanted.is_empty() {
                    p.parent = None;
                } else if !p.parent.is_some_and(|q| fits.iter().any(|(i, k)| *i == q && wanted.contains(k))) {
                    match first_parent {
                        Some(q) => p.parent = Some(q),
                        None => {
                            drop.insert(p.id);
                        }
                    }
                }
            }
            ph.places.retain(|p| !drop.contains(&p.id));
        }
        // Places without a city are worthless: start over from the implicit layout.
        if !ph.places.iter().any(|p| p.kind == PlaceKind::City) {
            ph.places.clear();
        }
        ph.homes.truncate(MAX_DEVICES);
        let mut seen = BTreeSet::new();
        let devices: BTreeSet<DeviceId> = self.devices.iter().map(|d| d.id).collect();
        let place_ids: BTreeSet<u32> = self.places().iter().map(|p| p.id).collect();
        let ph = &mut self.physical;
        ph.homes.retain(|h| devices.contains(&h.device) && place_ids.contains(&h.place) && seen.insert(h.device));
        for h in &mut ph.homes {
            h.x = finite(h.x);
            h.y = finite(h.y);
        }
        ph.lengths.truncate(MAX_LENGTHS);
        let mut seen = BTreeSet::new();
        let links = &self.links;
        ph.lengths.retain(|c| {
            (1..=MAX_CABLE_M).contains(&c.meters)
                && links.iter().any(|l| same_cable(l, c))
                && seen.insert(if c.a <= c.b { (c.a, c.b) } else { (c.b, c.a) })
        });
        ph.splices.truncate(MAX_SPLICED);
        let mut seen = BTreeSet::new();
        ph.splices.retain(|c| {
            (1..=MAX_SPLICES).contains(&c.count)
                && links.iter().any(|l| same_ends(l, c.a, c.b))
                && seen.insert(if c.a <= c.b { (c.a, c.b) } else { (c.b, c.a) })
        });
        let working = self.physical.working;
        if working.is_some_and(|w| !self.places().iter().any(|p| p.id == w && p.kind == PlaceKind::Closet)) {
            self.physical.working = None;
        }
        self.sanitize_furniture();
    }
}

fn to_meters(d: f32) -> u32 {
    if d.is_finite() { d.round().clamp(0.0, MAX_CABLE_M as f32) as u32 } else { MAX_CABLE_M }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceKind, file};

    fn lab() -> (Topology, DeviceId, DeviceId, DeviceId) {
        let mut t = Topology::new();
        let pc = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        let sw = t.add_device(DeviceKind::Switch, None, 100.0, 0.0).unwrap();
        let r = t.add_device(DeviceKind::Router, None, 200.0, 0.0).unwrap();
        t.connect(End { device: pc, port: 0 }, End { device: sw, port: 0 }).unwrap();
        t.connect(End { device: sw, port: 24 }, End { device: r, port: 0 }).unwrap();
        (t, pc, sw, r)
    }

    #[test]
    fn the_implicit_layout_holds_every_device_until_changed() {
        let (t, pc, ..) = lab();
        assert!(t.implicit_layout());
        assert_eq!(t.places().len(), 3);
        assert_eq!(t.home_of(pc).unwrap().place, DEFAULT_CLOSET);
        assert_eq!(t.place_path(Some(DEFAULT_CLOSET)).iter().map(|p| p.id).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert!(t.links.iter().all(|l| t.cable_length(l) == Length { meters: SAME_PLACE_M, manual: false }));
        // Saved without a physical section: old readers and round trips are unchanged.
        assert!(!file::to_json(&t).unwrap().contains("physical"));
    }

    #[test]
    fn places_nest_and_lengths_follow_them() {
        let (mut t, pc, sw, r) = lab();
        assert!(t.add_place(PlaceKind::Building, None, None, 0.0, 0.0).is_err());
        assert!(t.add_place(PlaceKind::Closet, None, Some(DEFAULT_CITY), 0.0, 0.0).is_err());
        let b2 = t.add_place(PlaceKind::Building, Some("Annex"), Some(DEFAULT_CITY), 300.0 + 140.0, 220.0).unwrap();
        let c2 = t.add_place(PlaceKind::Closet, None, Some(b2), 0.0, 0.0).unwrap();
        assert_eq!(t.place(c2).unwrap().name, "Closet 2");
        // A switch can't stand in a building's rooms; a PC can.
        assert!(t.set_home(sw, b2, 0.0, 0.0).is_err());
        assert!(t.set_home(pc, DEFAULT_CITY, 0.0, 0.0).is_err());
        t.set_home(pc, b2, 10.0, 10.0).unwrap();
        let l0 = t.links[0];
        assert_eq!(t.cable_length(&l0).meters, 140 + SAME_BUILDING_M);
        // Same building, different closets.
        t.set_home(r, DEFAULT_CLOSET, 0.0, 0.0).unwrap();
        t.set_home(sw, c2, 0.0, 0.0).unwrap();
        assert_eq!(t.cable_length(&t.links[0]).meters, SAME_BUILDING_M);
        assert_eq!(t.cable_length(&t.links[1]).meters, 140 + SAME_BUILDING_M);
        // Another city, 5 map units away.
        let city = t.add_place(PlaceKind::City, Some("Far"), None, 305.0, 220.0).unwrap();
        let b3 = t.add_place(PlaceKind::Building, None, Some(city), 0.0, 0.0).unwrap();
        let c3 = t.add_place(PlaceKind::Closet, None, Some(b3), 0.0, 0.0).unwrap();
        t.set_home(r, c3, 0.0, 0.0).unwrap();
        assert_eq!(t.cable_length(&t.links[1]).meters, 500 + 2 * SAME_BUILDING_M);
        // A hand-set length wins, either end names the cable, and clearing restores the rule.
        let l = t.set_cable_length(End { device: r, port: 0 }, Some(80)).unwrap();
        assert_eq!(l, Length { meters: 80, manual: true });
        assert!(t.set_cable_length(End { device: r, port: 0 }, Some(0)).is_err());
        assert!(t.set_cable_length(End { device: r, port: 3 }, Some(5)).is_err());
        t.set_cable_length(End { device: sw, port: 24 }, Some(90)).unwrap();
        assert_eq!(t.physical.lengths.len(), 1);
        t.set_cable_length(End { device: sw, port: 24 }, None).unwrap();
        assert!(!t.cable_length(&t.links[1]).manual);
        // Removing the far city sends its devices to the default closet; the last city stays.
        t.remove_place(city).unwrap();
        assert_eq!(t.home_of(r).unwrap().place, DEFAULT_CLOSET);
        assert!(t.remove_place(DEFAULT_CITY).is_err());
        // Removing a closet moves its devices into the building.
        t.remove_place(c2).unwrap();
        assert_eq!(t.home_of(sw).unwrap().place, b2);
        // Unplugging and removing forget lengths and homes.
        t.set_cable_length(End { device: pc, port: 0 }, Some(7)).unwrap();
        t.disconnect(End { device: pc, port: 0 });
        t.remove_device(sw).unwrap();
        assert!(t.physical.lengths.is_empty());
        assert!(t.physical.homes.iter().all(|h| h.device != sw));
        assert!(t.rename_place(b2, "  ").is_err());
        t.rename_place(b2, " Lab  ").unwrap();
        assert_eq!(t.place(b2).unwrap().name, "Lab");
    }

    #[test]
    fn physical_round_trips_and_hostile_files_are_repaired() {
        let (mut t, pc, sw, _) = lab();
        let b2 = t.add_place(PlaceKind::Building, Some("Annex"), Some(DEFAULT_CITY), 10.0, 10.0).unwrap();
        t.set_home(pc, b2, 4.0, 5.0).unwrap();
        t.set_cable_length(End { device: pc, port: 0 }, Some(12)).unwrap();
        let s = file::to_json(&t).unwrap();
        assert_eq!(file::from_json(&s).unwrap(), t);
        let mut v: serde_json::Value = serde_json::from_str(&s).unwrap();
        v["topology"]["physical"] = serde_json::json!({
            "places": [
                {"id": 1, "kind": "city", "name": "", "x": 1e30, "y": "nan", "parent": 9},
                {"id": 1, "kind": "city", "name": "dup"},
                {"id": 2, "kind": "building", "name": "B", "parent": 7},
                {"id": 3, "kind": "closet", "name": "C", "parent": 1},
                {"id": 4, "kind": "closet", "name": "D", "parent": 4},
            ],
            "homes": [
                {"device": pc.0, "place": 77},
                {"device": sw.0, "place": 3, "x": 1e30},
                {"device": sw.0, "place": 2},
                {"device": 9999, "place": 3}
            ],
            "lengths": [
                {"a": {"device": pc.0, "port": 0}, "b": {"device": sw.0, "port": 0}, "meters": 0},
                {"a": {"device": sw.0, "port": 0}, "b": {"device": pc.0, "port": 0}, "meters": 150},
                {"a": {"device": pc.0, "port": 0}, "b": {"device": sw.0, "port": 0}, "meters": 160},
                {"a": {"device": pc.0, "port": 5}, "b": {"device": sw.0, "port": 9}, "meters": 10}
            ]
        });
        // A non-number y fails the whole file, as any malformed field does.
        assert!(file::from_json(&v.to_string()).is_err());
        v["topology"]["physical"]["places"][0]["y"] = serde_json::json!(0);
        let r = file::from_json(&v.to_string()).unwrap();
        let ph = &r.physical;
        assert_eq!(ph.places.len(), 4);
        assert!(ph.places.iter().all(|p| !p.name.is_empty() && p.x.abs() <= 1e6));
        assert_eq!(ph.places[0].parent, None);
        assert_eq!(r.place(2).unwrap().parent, Some(1));
        assert_eq!(r.place(3).unwrap().parent, Some(2));
        assert_eq!(r.place(4).unwrap().parent, Some(2));
        assert_eq!(ph.homes.len(), 1);
        assert_eq!(r.home_of(sw).unwrap().place, 3);
        assert_eq!(r.home_of(pc).unwrap().place, r.default_place());
        assert_eq!(ph.lengths.len(), 1);
        assert_eq!(ph.lengths[0].meters, 150);
        // Places with no city at all: back to the implicit layout.
        v["topology"]["physical"]["places"] = serde_json::json!([{"id": 5, "kind": "closet", "name": "x", "parent": 5}]);
        assert!(file::from_json(&v.to_string()).unwrap().implicit_layout());
        // Oversized lists are capped.
        let many: Vec<serde_json::Value> =
            (1..=MAX_PLACES as u32 + 50).map(|i| serde_json::json!({"id": i, "kind": "city", "name": format!("C{i}")})).collect();
        v["topology"]["physical"]["places"] = serde_json::Value::Array(many);
        assert_eq!(file::from_json(&v.to_string()).unwrap().physical.places.len(), MAX_PLACES);
    }

    #[test]
    fn place_caps_hold() {
        let mut t = Topology::new();
        for _ in 0..MAX_PLACES - 3 {
            t.add_place(PlaceKind::City, None, None, 0.0, 0.0).unwrap();
        }
        assert!(t.add_place(PlaceKind::City, None, None, 0.0, 0.0).is_err());
    }

    #[test]
    fn a_fiber_link_is_measured_against_its_transceivers_power_budget() {
        use crate::cable::Cable;
        let mut t = Topology::new();
        let a = t.add_model(crate::models::find("Switch-24-SFP").unwrap(), None, 0.0, 0.0).unwrap();
        let b = t.add_model(crate::models::find("Switch-24-SFP").unwrap(), None, 100.0, 0.0).unwrap();
        let (ea, eb) = (End { device: a, port: 24 }, End { device: b, port: 24 });
        t.connect_with(ea, eb, Cable::FiberMulti).unwrap();
        let l = *t.link_at(ea).unwrap();
        // 3 m of multi-mode: two connectors, nothing else, well inside the budget.
        let fb = t.fiber_budget(&l).unwrap();
        assert_eq!((fb.connectors, fb.splices, fb.state()), (2, 0, BudgetState::Ok));
        // 550 m (the reach of the cable) leaves under a decibel.
        t.set_cable_length(ea, Some(550)).unwrap();
        assert_eq!(t.fiber_budget(&l).unwrap().state(), BudgetState::Degraded);
        // Two splices at 550 m take it over the budget; clearing them brings it back.
        assert_eq!(t.set_splices(ea, 2).unwrap(), 2);
        assert_eq!(t.splices_on(&l), 2);
        assert_eq!(t.fiber_budget(&l).unwrap().state(), BudgetState::Over);
        t.set_splices(eb, 0).unwrap();
        assert!(t.physical.splices.is_empty());
        // Single-mode over single-mode fiber has a bigger budget and less loss a kilometre.
        for e in [ea, eb] {
            t.device_mut(e.device).unwrap().ports[e.port].single_mode = true;
        }
        t.links[0].cable = Cable::FiberSingle;
        let l = *t.link_at(ea).unwrap();
        t.set_cable_length(ea, Some(5_000)).unwrap();
        let fb = t.fiber_budget(&l).unwrap();
        assert_eq!((fb.budget_db, fb.state()), (SINGLE_MODE_BUDGET_DB, BudgetState::Ok));
        t.set_splices(ea, 3).unwrap();
        t.set_cable_length(ea, Some(10_000)).unwrap();
        assert_eq!(t.fiber_budget(&l).unwrap().state(), BudgetState::Degraded);
        t.set_splices(ea, 4).unwrap();
        assert_eq!(t.fiber_budget(&l).unwrap().state(), BudgetState::Over);
        // Refusals: too many, none on copper.
        assert!(t.set_splices(ea, MAX_SPLICES + 1).is_err());
        let (c, d) = (t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap(), t.add_device(DeviceKind::Switch, None, 0.0, 0.0).unwrap());
        t.connect(End { device: c, port: 0 }, End { device: d, port: 0 }).unwrap();
        assert!(t.set_splices(End { device: c, port: 0 }, 1).is_err());
        assert!(t.fiber_budget(t.link_at(End { device: c, port: 0 }).unwrap()).is_none());
        // Removing the cable forgets its splices.
        t.disconnect(ea).unwrap();
        assert!(t.physical.splices.is_empty());
    }
}
