//! A small cellular network: smartphones and tablets with a cellular radio, cell towers, and the
//! central office server that admits the devices and gives them addresses.
//!
//! Who is covered, who gets a sector on which tower, whether the central office admits the device
//! and which address it gets are all worked out from the lab, as Wi-Fi association is
//! ([`Topology::wifi_attempt`]); [`Topology::cell_refresh`] writes the result down (the address on
//! the device's `Cellular0` port) and keeps it in [`Topology::cell_links`], which the simulator
//! reads on every hop. The simulator then shows the attach exchange (see `netlabcraft_wire::cell`)
//! and carries the data frames.
//!
//! Model, honestly: this is not 3GPP. A device attaches to the nearest tower in range that has a
//! free sector (a tower has six); the tower bridges its sectors to its backhaul port; the
//! backhaul reaches the central office server through cables, hubs and switches (VLANs are not
//! looked at); the server admits the device when its pool, data plan and APN allow it.

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

use netlabcraft_core::{DeviceId, Ipv4Net};
use serde::{Deserialize, Serialize};

use crate::physical::{CITY_UNIT_M, PlaceKind};
use crate::{Device, DeviceKind, End, Topology, ifname};

/// How far a tower reaches on the logical map, when the lab has no physical layout.
pub const CELL_RANGE: f32 = 1000.0;
/// How far a tower reaches in the physical view: metres on a city map.
pub const CELL_RANGE_M: u32 = 600;
/// Longest access point name.
pub const MAX_APN: usize = 32;
/// Most devices the backhaul search visits.
const MAX_BACKHAUL_VISITS: usize = 256;

/// Cellular settings of a device: on a smartphone or tablet the radio's, on a central office
/// server the provider's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellConf {
    /// Phones and tablets: Cellular Data switched off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub data_off: bool,
    /// Phones: the access point name asked for. Central office: the one it admits (empty: any).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub apn: String,
    /// Central office: the network the devices' addresses come from; the server's own interface
    /// in it is their gateway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool: Option<Ipv4Net>,
    /// Central office: the data plan is switched off (every attach is refused).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub plan_off: bool,
}

impl CellConf {
    pub fn is_default(&self) -> bool {
        *self == CellConf::default()
    }
}

/// Why the central office (or the path to it) refused a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No central office server answers behind the tower's backhaul.
    NoCore,
    /// The server has no pool, or none of its interfaces is in it.
    NoPool,
    /// The data plan is off.
    Plan,
    /// The server admits another APN.
    Apn,
    /// Every address of the pool is taken.
    Exhausted,
}

impl Refusal {
    pub fn text(self) -> &'static str {
        match self {
            Refusal::NoCore => "no central office server answers behind this tower",
            Refusal::NoPool => "the central office server has no address pool (or no interface in it)",
            Refusal::Plan => "the data plan is off",
            Refusal::Apn => "the APN is not accepted",
            Refusal::Exhausted => "the address pool is exhausted",
        }
    }
}

/// What the central office gave a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub address: Ipv4Net,
    pub gateway: Ipv4Addr,
    pub apn: String,
}

/// A device's radio link to a tower, and what the core made of its attach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellLink {
    pub tower: DeviceId,
    /// The tower's port the device sits on.
    pub sector: usize,
    /// Signal strength, 1 (weak) to 4 (strong).
    pub bars: u8,
    /// The server's interface the attach reaches (when one answers).
    pub core: Option<End>,
    pub outcome: Result<Lease, Refusal>,
}

/// Why a device has no cellular link at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoLink {
    /// Not a cellular device, or the radio is shut down or the device is dark.
    Unavailable,
    /// Cellular Data is off.
    DataOff,
    /// No tower in range.
    NoService,
    /// Towers in range have no free sector.
    TowerFull,
}

impl NoLink {
    pub fn text(self) -> &'static str {
        match self {
            NoLink::Unavailable => "the cellular radio is not available",
            NoLink::DataOff => "Cellular Data is off",
            NoLink::NoService => "no service: no cell tower in range",
            NoLink::TowerFull => "no service: every tower in range is full",
        }
    }
}

impl Device {
    pub fn is_tower(&self) -> bool {
        self.catalogue_model().tower()
    }

    pub fn is_central_office(&self) -> bool {
        self.catalogue_model().central_office()
    }

    /// The index of a phone's or tablet's cellular port.
    pub fn cell_port(&self) -> Option<usize> {
        if !self.catalogue_model().touch() {
            return None;
        }
        self.ports.iter().position(|p| ifname::is_cellular(&p.name))
    }

    /// Phones and tablets made before the cellular radio have no `Cellular0` port: gives them one.
    pub(crate) fn ensure_cellular_port(&mut self) {
        if !self.catalogue_model().touch() || self.cell_port().is_some() {
            return;
        }
        let fresh = Device::of_model(self.id, self.catalogue_model(), String::new(), 0.0, 0.0);
        if let Some(p) = fresh.ports.into_iter().find(|p| ifname::is_cellular(&p.name)) {
            self.ports.push(p);
        }
    }

    /// Cleans hostile cellular settings.
    pub(crate) fn sanitize_cellular(&mut self) {
        self.ensure_cellular_port();
        self.cell.apn = self.cell.apn.chars().filter(|c| c.is_ascii_graphic()).take(MAX_APN).collect();
        let m = self.catalogue_model();
        if !m.touch() {
            self.cell.data_off = false;
        }
        if !m.central_office() {
            self.cell.pool = None;
            self.cell.plan_off = false;
            if !m.touch() {
                self.cell.apn.clear();
            }
        }
        if let Some(p) = self.cell.pool {
            self.cell.pool = Some(Ipv4Net::new(p.addr, p.prefix.clamp(8, 30)).network());
        }
    }
}

/// A device's place on a city map: (city, x, y) in metres. A device in a building is where the
/// building's icon is; a device in a closet, where its building is.
fn city_position(t: &Topology, d: &Device) -> Option<(u32, f32, f32)> {
    let h = t.home_of(d.id)?;
    let place = t.place(h.place)?;
    match place.kind {
        PlaceKind::City => Some((place.id, h.x, h.y)),
        PlaceKind::Building | PlaceKind::Closet => {
            let b = t.building_of(place.id)?;
            Some((b.parent?, b.x, b.y))
        }
    }
}

fn bars_for(fraction: f32) -> u8 {
    match fraction {
        f if f < 0.25 => 4,
        f if f < 0.5 => 3,
        f if f < 0.75 => 2,
        _ => 1,
    }
}

impl Topology {
    /// The signal a device gets from a tower: its bars, and the distance for ranking, when in
    /// range. Without a physical layout the logical map decides; with one, the tower and the
    /// device must be in the same city and within [`CELL_RANGE_M`] metres on its map.
    pub fn cell_signal(&self, tower: &Device, device: &Device) -> Option<(u8, f32)> {
        let (dist, range) = if self.implicit_layout() {
            (((tower.x - device.x).powi(2) + (tower.y - device.y).powi(2)).sqrt(), CELL_RANGE)
        } else {
            let (ct, xt, yt) = city_position(self, tower)?;
            let (cd, xd, yd) = city_position(self, device)?;
            if ct != cd {
                return None;
            }
            (((xt - xd).powi(2) + (yt - yd).powi(2)).sqrt() * CITY_UNIT_M, CELL_RANGE_M as f32)
        };
        (dist.is_finite() && dist <= range).then(|| (bars_for(dist / range), dist))
    }

    /// The central office interfaces a tower's backhaul reaches through cables, hubs and
    /// switches (VLANs are not looked at).
    fn backhaul_cores(&self, tower: &Device) -> Vec<End> {
        let Some(backhaul) = tower.ports.iter().position(|p| !ifname::is_cellular(&p.name)) else { return Vec::new() };
        let mut out = Vec::new();
        let mut seen = BTreeSet::from([tower.id]);
        let mut todo = vec![End { device: tower.id, port: backhaul }];
        while let Some(from) = todo.pop() {
            let Some(link) = self.link_at(from) else { continue };
            let far = if link.a == from { link.b } else { link.a };
            let Some(fd) = self.device(far.device) else { continue };
            if fd.ports.get(far.port).is_none_or(|p| p.shutdown || p.err_disabled) || fd.powered_off {
                continue;
            }
            if fd.is_central_office() {
                out.push(far);
            } else if (fd.kind == DeviceKind::Switch || (fd.kind == DeviceKind::Hub && !fd.catalogue_model().cloud() && !fd.is_tower()))
                && seen.insert(fd.id)
                && seen.len() <= MAX_BACKHAUL_VISITS
            {
                todo.extend((0..fd.ports.len()).filter(|p| *p != far.port).map(|port| End { device: fd.id, port }));
            }
        }
        out
    }

    /// Works out every cellular device's link and attach from the lab.
    fn compute_cell_links(&self) -> (BTreeMap<DeviceId, CellLink>, BTreeMap<DeviceId, NoLink>) {
        let mut links = BTreeMap::new();
        let mut none = BTreeMap::new();
        let towers: Vec<&Device> = self.devices.iter().filter(|d| d.is_tower() && !self.is_dark(d.id)).collect();
        let mut phones: Vec<&Device> = self.devices.iter().filter(|d| d.cell_port().is_some()).collect();
        phones.sort_by_key(|d| d.id);
        // Sectors in use per tower.
        let mut used: BTreeMap<DeviceId, usize> = BTreeMap::new();
        // The pre-admission result per device: (link without an address, core interface, refusal).
        let mut pending: Vec<(DeviceId, DeviceId, usize, u8, Option<End>, Result<(), Refusal>)> = Vec::new();
        let mut cores: BTreeMap<DeviceId, Vec<End>> = BTreeMap::new();
        for ph in phones {
            let Some(cp) = ph.cell_port() else { continue };
            if ph.ports.get(cp).is_none_or(|p| p.shutdown) || self.is_dark(ph.id) {
                none.insert(ph.id, NoLink::Unavailable);
                continue;
            }
            if ph.cell.data_off {
                none.insert(ph.id, NoLink::DataOff);
                continue;
            }
            let mut ranked: Vec<(f32, u8, &Device)> = towers.iter().filter_map(|t| self.cell_signal(t, ph).map(|(b, d)| (d, b, *t))).collect();
            ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.id.cmp(&b.2.id)));
            if ranked.is_empty() {
                none.insert(ph.id, NoLink::NoService);
                continue;
            }
            let mut placed = false;
            for (_, bars, tw) in ranked {
                let sector_ports: Vec<usize> =
                    (0..tw.ports.len()).filter(|p| tw.ports.get(*p).is_some_and(|x| ifname::is_cellular(&x.name) && !x.shutdown)).collect();
                let n = used.entry(tw.id).or_insert(0);
                let Some(sector) = sector_ports.get(*n).copied() else { continue };
                *n += 1;
                let reached = cores.entry(tw.id).or_insert_with(|| self.backhaul_cores(tw)).clone();
                let (core, verdict) = self.admission(ph, &reached);
                pending.push((ph.id, tw.id, sector, bars, core, verdict));
                placed = true;
                break;
            }
            if !placed {
                none.insert(ph.id, NoLink::TowerFull);
            }
        }
        // Addresses: the admitted devices of each server interface take its pool in id order.
        let mut next: BTreeMap<End, u32> = BTreeMap::new();
        for (id, tower, sector, bars, core, verdict) in pending {
            let outcome = match (verdict, core) {
                (Err(r), _) => Err(r),
                (Ok(()), None) => Err(Refusal::NoCore),
                (Ok(()), Some(c)) => self.lease_at(c, &mut next, self.device(id)),
            };
            links.insert(id, CellLink { tower, sector, bars, core, outcome });
        }
        (links, none)
    }

    /// Whether a server behind `reached` admits the device (before addresses are counted), and
    /// the interface it would do it on.
    fn admission(&self, phone: &Device, reached: &[End]) -> (Option<End>, Result<(), Refusal>) {
        let mut why = Refusal::NoCore;
        for e in reached {
            let Some(co) = self.device(e.device) else { continue };
            let Some(pool) = co.cell.pool else {
                why = Refusal::NoPool;
                continue;
            };
            let in_pool = co.ports.get(e.port).and_then(|p| p.ip).is_some_and(|ip| pool.contains(ip.addr) && ip.prefix == pool.prefix);
            if !in_pool {
                why = Refusal::NoPool;
                continue;
            }
            if co.cell.plan_off {
                return (Some(*e), Err(Refusal::Plan));
            }
            if !co.cell.apn.is_empty() && !co.cell.apn.eq_ignore_ascii_case(&phone.cell.apn) {
                return (Some(*e), Err(Refusal::Apn));
            }
            return (Some(*e), Ok(()));
        }
        (None, Err(why))
    }

    /// The next free address of the pool behind a server interface.
    fn lease_at(&self, core: End, next: &mut BTreeMap<End, u32>, phone: Option<&Device>) -> Result<Lease, Refusal> {
        let co = self.device(core.device).ok_or(Refusal::NoCore)?;
        let pool = co.cell.pool.ok_or(Refusal::NoPool)?;
        let gw = co.ports.get(core.port).and_then(|p| p.ip).ok_or(Refusal::NoPool)?;
        let k = next.entry(core).or_insert(0);
        let base = u32::from(pool.network().addr);
        let last = u32::from(pool.broadcast());
        // The k-th address after the network address that is not the gateway; hosts only.
        let mut skip = *k;
        let mut cand = base.saturating_add(1);
        let mut found = None;
        // Bounded: at most 65 536 candidates (a /16) and never past the broadcast address.
        for _ in 0..65_536u32 {
            if cand >= last {
                break;
            }
            if Ipv4Addr::from(cand) != gw.addr {
                if skip == 0 {
                    found = Some(cand);
                    break;
                }
                skip -= 1;
            }
            cand = cand.saturating_add(1);
        }
        let a = found.ok_or(Refusal::Exhausted)?;
        *k = k.saturating_add(1);
        Ok(Lease {
            address: Ipv4Net::new(Ipv4Addr::from(a), pool.prefix),
            gateway: gw.addr,
            apn: phone.map(|p| p.cell.apn.clone()).unwrap_or_default(),
        })
    }

    /// Recomputes every cellular link and writes each admitted device's address on its
    /// `Cellular0` port (and clears it on the others). Call after any change to the lab.
    pub fn cell_refresh(&mut self) {
        if self.cell_links.is_empty() && !self.devices.iter().any(|d| d.cell_port().is_some() || d.is_tower()) {
            return;
        }
        let (links, none) = self.compute_cell_links();
        self.cell_none = none;
        for d in &mut self.devices {
            let Some(cp) = d.cell_port() else { continue };
            let lease = links.get(&d.id).and_then(|l| l.outcome.as_ref().ok());
            if let Some(p) = d.ports.get_mut(cp) {
                p.ip = lease.map(|l| l.address);
                p.dhcp_router = lease.map(|l| l.gateway);
            }
        }
        self.cell_links = links;
    }

    /// A device's cellular link, if it has one.
    pub fn cell_link(&self, device: DeviceId) -> Option<&CellLink> {
        self.cell_links.get(&device)
    }

    /// Why a device has no cellular link.
    pub fn cell_no_link(&self, device: DeviceId) -> Option<NoLink> {
        if self.cell_links.contains_key(&device) {
            return None;
        }
        self.cell_none.get(&device).copied().or_else(|| self.device(device).and_then(|d| d.cell_port()).map(|_| NoLink::Unavailable))
    }

    /// The far end of a cellular radio link: a device's cellular port to its tower sector, and a
    /// tower sector back to the device on it.
    pub fn cell_peer(&self, end: End) -> Option<End> {
        if self.cell_links.is_empty() {
            return None;
        }
        if let Some(l) = self.cell_links.get(&end.device) {
            let cp = self.device(end.device)?.cell_port()?;
            return (cp == end.port).then_some(End { device: l.tower, port: l.sector });
        }
        let d = self.device(end.device)?;
        if !d.is_tower() {
            return None;
        }
        self.cell_links.iter().find(|(_, l)| l.tower == end.device && l.sector == end.port).and_then(|(id, _)| {
            let cp = self.device(*id)?.cell_port()?;
            Some(End { device: *id, port: cp })
        })
    }

    /// The devices attached to a tower, with their links, by sector.
    pub fn tower_clients(&self, tower: DeviceId) -> Vec<(DeviceId, &CellLink)> {
        let mut v: Vec<(DeviceId, &CellLink)> = self.cell_links.iter().filter(|(_, l)| l.tower == tower).map(|(id, l)| (*id, l)).collect();
        v.sort_by_key(|(_, l)| l.sector);
        v
    }

    /// The devices a central office interface has admitted, in address order.
    pub fn core_clients(&self, device: DeviceId) -> Vec<(DeviceId, &CellLink, &Lease)> {
        let mut v: Vec<(DeviceId, &CellLink, &Lease)> = self
            .cell_links
            .iter()
            .filter_map(|(id, l)| match (&l.core, &l.outcome) {
                (Some(c), Ok(lease)) if c.device == device => Some((*id, l, lease)),
                _ => None,
            })
            .collect();
        v.sort_by_key(|(_, _, lease)| lease.address.addr);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models;

    /// A phone, a tower and a central office on one backhaul cable.
    fn lab() -> (Topology, DeviceId, DeviceId, DeviceId) {
        let mut t = Topology::new();
        let tw = t.add_model(models::find("Cell Tower").unwrap(), None, 100.0, 0.0).unwrap();
        let co = t.add_model(models::find("Central Office Server").unwrap(), None, 300.0, 0.0).unwrap();
        let ph = t.add_model(models::find("Smartphone").unwrap(), None, 0.0, 0.0).unwrap();
        t.connect(End { device: tw, port: 0 }, End { device: co, port: 0 }).unwrap();
        if let Some(d) = t.device_mut(co) {
            d.ports[0].shutdown = false;
            d.ports[0].ip = Some(Ipv4Net::new("10.64.0.1".parse().unwrap(), 24));
            d.cell.pool = Some(Ipv4Net::new("10.64.0.0".parse().unwrap(), 24));
        }
        t.cell_refresh();
        (t, tw, co, ph)
    }

    #[test]
    fn a_phone_in_range_attaches_and_gets_the_first_pool_address() {
        let (t, tw, co, ph) = lab();
        let l = t.cell_link(ph).unwrap();
        assert_eq!(l.tower, tw);
        assert_eq!(l.outcome.as_ref().unwrap().address, Ipv4Net::new("10.64.0.2".parse().unwrap(), 24));
        assert_eq!(l.outcome.as_ref().unwrap().gateway, "10.64.0.1".parse::<Ipv4Addr>().unwrap());
        let cp = t.device(ph).unwrap().cell_port().unwrap();
        assert_eq!(t.device(ph).unwrap().ports[cp].ip.unwrap().addr, "10.64.0.2".parse::<Ipv4Addr>().unwrap());
        assert_eq!(t.cell_peer(End { device: ph, port: cp }), Some(End { device: tw, port: l.sector }));
        assert_eq!(t.cell_peer(End { device: tw, port: l.sector }), Some(End { device: ph, port: cp }));
        assert_eq!(t.core_clients(co).len(), 1);
        assert!(l.bars >= 3);
    }

    #[test]
    fn data_off_range_plan_apn_and_pool_decide_the_attach() {
        let (mut t, _, co, ph) = lab();
        t.device_mut(ph).unwrap().cell.data_off = true;
        t.cell_refresh();
        assert_eq!(t.cell_no_link(ph), Some(NoLink::DataOff));
        assert!(t.device(ph).unwrap().ports[1].ip.is_none());
        t.device_mut(ph).unwrap().cell.data_off = false;
        t.device_mut(ph).unwrap().x = 5000.0;
        t.cell_refresh();
        assert_eq!(t.cell_no_link(ph), Some(NoLink::NoService));
        t.device_mut(ph).unwrap().x = 0.0;
        t.device_mut(co).unwrap().cell.plan_off = true;
        t.cell_refresh();
        assert_eq!(t.cell_link(ph).unwrap().outcome, Err(Refusal::Plan));
        assert!(t.device(ph).unwrap().ports[1].ip.is_none());
        t.device_mut(co).unwrap().cell.plan_off = false;
        t.device_mut(co).unwrap().cell.apn = "internet".into();
        t.cell_refresh();
        assert_eq!(t.cell_link(ph).unwrap().outcome, Err(Refusal::Apn));
        t.device_mut(ph).unwrap().cell.apn = "INTERNET".into();
        t.cell_refresh();
        assert!(t.cell_link(ph).unwrap().outcome.is_ok());
        t.device_mut(co).unwrap().cell.pool = None;
        t.cell_refresh();
        assert_eq!(t.cell_link(ph).unwrap().outcome, Err(Refusal::NoPool));
    }

    #[test]
    fn a_tower_has_six_sectors_and_a_pool_runs_out() {
        let (mut t, _, co, _) = lab();
        for i in 0..7 {
            t.add_model(models::find("Tablet").unwrap(), None, f32::from(i as u8), 10.0).unwrap();
        }
        t.cell_refresh();
        // One phone plus seven tablets: six sectors, two devices without service.
        assert_eq!(t.cell_links.len(), 6);
        assert_eq!(t.cell_none.values().filter(|n| **n == NoLink::TowerFull).count(), 2);
        // A /29 pool has six hosts: the gateway takes one, so five devices fit.
        t.device_mut(co).unwrap().ports[0].ip = Some(Ipv4Net::new("10.64.0.1".parse().unwrap(), 29));
        t.device_mut(co).unwrap().cell.pool = Some(Ipv4Net::new("10.64.0.0".parse().unwrap(), 29));
        t.cell_refresh();
        assert_eq!(t.cell_links.values().filter(|l| l.outcome.is_ok()).count(), 5);
        assert_eq!(t.cell_links.values().filter(|l| l.outcome == Err(Refusal::Exhausted)).count(), 1);
    }

    #[test]
    fn old_phones_get_a_cellular_port_and_hostile_settings_are_cleaned() {
        let (mut t, _, _, ph) = lab();
        t.device_mut(ph).unwrap().ports.truncate(1);
        t.device_mut(ph).unwrap().cell.apn = "a b\u{7}".repeat(40);
        t.sanitize();
        let d = t.device(ph).unwrap();
        assert_eq!(d.ports.len(), 2);
        assert!(d.cell.apn.len() <= MAX_APN && d.cell.apn.chars().all(|c| c.is_ascii_graphic()));
        assert!(t.cell_link(ph).is_some());
    }
}
