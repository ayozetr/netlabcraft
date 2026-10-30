//! Hardware: the cards in a modular router's bays and the power switch.
//!
//! A modular router (`models::Model::slots`) leaves the factory with empty bays. A card goes
//! in, or comes out, only while the device is switched off, as on real equipment. Its ports
//! are named after the bay (`Serial0/1/0`) and join the device's port list after the built-in
//! ports, in bay order. Cables keep following the ports they were plugged into; those on a
//! card that comes out are unplugged.

use std::collections::BTreeMap;

use netlabcraft_core::{DeviceId, MacAddr};

use crate::{Device, ModelError, Result, Topology, models};

impl Device {
    /// The card in a bay, if any.
    pub fn module_in(&self, slot: &str) -> Option<&'static models::Module> {
        self.modules.get(slot).and_then(|id| models::module(id))
    }

    /// The bay a port sits in (`Serial0/1/0` → `0/1`), for card ports.
    pub fn port_slot(&self, port: &str) -> Option<&'static str> {
        let slots = self.catalogue_model().slots();
        slots.iter().copied().find(|s| self.module_in(s).is_some_and(|m| m.port_names(s).iter().any(|n| n == port)))
    }

    /// The ports this device has: the model's, then each card's in bay order.
    fn hardware_port_names(&self) -> Vec<String> {
        let model = self.catalogue_model();
        let mut names = model.port_names();
        for slot in model.slots() {
            if let Some(m) = self.module_in(slot) {
                names.extend(m.port_names(slot));
            }
        }
        names
    }

    /// Drops cards in bays the model doesn't have, or of unknown types (hostile files).
    pub(crate) fn sanitize_modules(&mut self) {
        let slots = self.catalogue_model().slots();
        self.modules.retain(|slot, id| slots.contains(&slot.as_str()) && models::module(id).is_some());
    }

    /// Whether the device keeps a running configuration apart from a saved one (routers and
    /// switches): switching it off loses what wasn't saved. Hosts and home routers keep their
    /// settings, as on their real counterparts.
    pub fn has_running_config(&self) -> bool {
        matches!(self.kind, crate::DeviceKind::Router | crate::DeviceKind::Switch) && !self.catalogue_model().home_router()
    }

    /// Switches the device off: every port goes dark and the running configuration is lost
    /// (the saved one, and the VLAN database, stay).
    pub fn power_off(&mut self) {
        if self.has_running_config() {
            self.reset_config();
        }
        self.powered_off = true;
    }
}

/// The bay a person typed: `0/1` as listed, or just its last number (`1`).
pub fn find_slot(model: &models::Model, typed: &str) -> Option<&'static str> {
    let typed = typed.trim();
    model.slots().iter().copied().find(|s| s.eq_ignore_ascii_case(typed) || s.rsplit('/').next() == Some(typed))
}

/// The MAC address of port `n` of the card in bay number `bay`: apart from the built-in
/// ports' addresses (which count from 0).
fn card_mac(device: DeviceId, bay: usize, n: usize) -> MacAddr {
    let v = 0x100usize.saturating_add(bay.saturating_mul(16)).saturating_add(n);
    MacAddr::local(device, u16::try_from(v).unwrap_or(u16::MAX))
}

impl Topology {
    /// Puts a card in a bay (`Some(module id)`) or takes the bay's card out (`None`). The
    /// device must be switched off. Returns the ports that appeared or went away.
    pub fn set_module(&mut self, id: DeviceId, slot: &str, module: Option<&str>) -> Result<Vec<String>> {
        let d = self.device(id).ok_or_else(|| ModelError::NoDevice(id.to_string()))?;
        let model = d.catalogue_model();
        if model.slots().is_empty() {
            return Err(ModelError::Invalid(format!("{} ({}) has no module bays", d.name, model.label)));
        }
        let Some(bay) = find_slot(model, slot) else {
            return Err(ModelError::Invalid(format!("{} has no bay {slot} (bays: {})", d.name, model.slots().join(", "))));
        };
        if !d.powered_off {
            return Err(ModelError::Invalid(format!("{} is switched on: switch it off before fitting or removing a card", d.name)));
        }
        let card = match module {
            Some(m) => {
                let card = models::module(m).ok_or_else(|| {
                    ModelError::Invalid(format!(
                        "unknown card `{m}` (cards: {})",
                        models::MODULES.iter().map(|c| c.id).collect::<Vec<_>>().join(", ")
                    ))
                })?;
                if let Some(old) = d.module_in(bay) {
                    return Err(ModelError::Invalid(format!("bay {bay} of {} already holds a {}", d.name, old.label)));
                }
                Some(card)
            }
            None => {
                if d.module_in(bay).is_none() {
                    return Err(ModelError::Invalid(format!("bay {bay} of {} is empty", d.name)));
                }
                None
            }
        };
        let mut nd = d.clone();
        let changed = match card {
            Some(c) => {
                nd.modules.insert(bay.to_string(), c.id.to_string());
                c.port_names(bay)
            }
            None => nd.modules.remove(bay).and_then(|m| models::module(&m)).map(|c| c.port_names(bay)).unwrap_or_default(),
        };
        // The new port list: ports that stay keep their settings and addresses.
        let blank = Device::of_model(id, model, nd.name.clone(), 0.0, 0.0).ports.into_iter().next();
        let mut ports = Vec::new();
        let mut moved: BTreeMap<usize, usize> = BTreeMap::new();
        for name in nd.hardware_port_names() {
            match nd.ports.iter().position(|p| p.name == name) {
                Some(old) => {
                    moved.insert(old, ports.len());
                    if let Some(p) = nd.ports.get(old) {
                        ports.push(p.clone());
                    }
                }
                None => {
                    let Some(b) = &blank else { continue };
                    // `Serial0/1/0`: bay `0/1`, port 0.
                    let (bay_path, n) = name.rsplit_once('/').unwrap_or((name.as_str(), "0"));
                    let bay_no = model.slots().iter().position(|s| bay_path.ends_with(s)).unwrap_or(0);
                    let mac = card_mac(id, bay_no, n.parse::<usize>().unwrap_or(0) % 16);
                    ports.push(crate::Port { name, mac, ..b.clone() });
                }
            }
        }
        nd.ports = ports;
        if let Some(slot) = self.device_mut(id) {
            *slot = nd;
        }
        // Cables follow their ports; those on ports that went away are unplugged.
        let remap = |e: crate::End| if e.device == id { moved.get(&e.port).map(|&p| crate::End { device: id, port: p }) } else { Some(e) };
        let mut links = Vec::new();
        for l in std::mem::take(&mut self.links) {
            if let (Some(a), Some(b)) = (remap(l.a), remap(l.b)) {
                links.push(crate::Link { a, b, cable: l.cable });
            }
        }
        self.links = links;
        let mut lengths = Vec::new();
        for c in std::mem::take(&mut self.physical.lengths) {
            if let (Some(a), Some(b)) = (remap(c.a), remap(c.b)) {
                lengths.push(crate::physical::CableLength { a, b, meters: c.meters });
            }
        }
        self.physical.lengths = lengths;
        self.prune_physical();
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use crate::{End, Topology, models};

    #[test]
    fn cards_go_in_while_off_and_cables_follow_their_ports() {
        let mut t = Topology::new();
        let r = t.add_model(models::find("Router-M3").unwrap(), None, 0.0, 0.0).unwrap();
        let r2 = t.add_model(models::find("Router-M2").unwrap(), None, 0.0, 0.0).unwrap();
        assert_eq!(t.device(r).unwrap().ports.len(), 3);
        // Switched on: refused.
        assert!(t.set_module(r, "0/1", Some("Card-2S")).is_err());
        t.device_mut(r).unwrap().power_off();
        t.device_mut(r2).unwrap().power_off();
        assert!(t.set_module(r, "0/9", Some("Card-2S")).is_err());
        assert!(t.set_module(r, "0/1", Some("nope")).is_err());
        assert!(t.set_module(r, "0/1", None).is_err());
        assert_eq!(t.set_module(r, "0/1", Some("Card-2S")).unwrap(), ["Serial0/1/0", "Serial0/1/1"]);
        assert!(t.set_module(r, "1", Some("Card-1GE")).is_err(), "bay taken");
        t.set_module(r, "0", Some("Card-1GE")).unwrap();
        t.set_module(r2, "0/0", Some("Card-2S")).unwrap();
        let names: Vec<String> = t.device(r).unwrap().ports.iter().map(|p| p.name.clone()).collect();
        assert_eq!(names, ["GigabitEthernet0/0", "GigabitEthernet0/1", "GigabitEthernet0/2", "GigabitEthernet0/0/0", "Serial0/1/0", "Serial0/1/1"]);
        let macs: std::collections::BTreeSet<_> = t.device(r).unwrap().ports.iter().map(|p| p.mac).collect();
        assert_eq!(macs.len(), 6);
        assert!(t.device(r).unwrap().ports[5].shutdown, "router ports start shut down");
        // A serial cable on the card, then the Gigabit card comes out: the cable follows.
        let s011 = t.device(r).unwrap().port_index("s0/1/1").unwrap();
        let far = t.device(r2).unwrap().port_index("s0/0/0").unwrap();
        t.connect(End { device: r, port: s011 }, End { device: r2, port: far }).unwrap();
        t.set_cable_length(End { device: r, port: s011 }, Some(30)).ok();
        t.set_module(r, "0/0", None).unwrap();
        let s011 = t.device(r).unwrap().port_index("s0/1/1").unwrap();
        assert_eq!(s011, 4);
        assert_eq!(t.peer(End { device: r, port: s011 }), Some(End { device: r2, port: far }));
        assert_eq!(t.device(r).unwrap().port_slot("Serial0/1/1"), Some("0/1"));
        // Out comes the serial card: its cable is unplugged.
        t.set_module(r, "0/1", None).unwrap();
        assert!(t.links.is_empty() && t.physical.lengths.is_empty());
        assert_eq!(t.device(r).unwrap().ports.len(), 3);
        // A fixed model has no bays.
        let r3 = t.add_device(crate::DeviceKind::Router, None, 0.0, 0.0).unwrap();
        t.device_mut(r3).unwrap().power_off();
        assert!(t.set_module(r3, "0/0", Some("Card-2S")).is_err());
    }

    #[test]
    fn power_off_loses_the_running_configuration_and_files_keep_cards() {
        let mut t = Topology::new();
        let r = t.add_model(models::find("Router-M2").unwrap(), None, 0.0, 0.0).unwrap();
        let d = t.device_mut(r).unwrap();
        d.ports[0].ip = Some("10.0.0.1/24".parse().unwrap());
        d.power_off();
        assert!(d.powered_off && d.ports[0].ip.is_none());
        t.set_module(r, "0/1", Some("Card-2S")).unwrap();
        t.device_mut(r).unwrap().ports[3].ip = Some("10.9.0.1/30".parse().unwrap());
        t.device_mut(r).unwrap().reset_config();
        assert!(t.device(r).unwrap().ports[3].ip.is_none() && t.device(r).unwrap().ports[3].name == "Serial0/1/1");
        let text = crate::file::to_json(&t).unwrap();
        let mut back = crate::file::from_json(&text).unwrap();
        assert_eq!(back.device(r).unwrap().modules.get("0/1").map(String::as_str), Some("Card-2S"));
        assert!(back.device(r).unwrap().powered_off);
        back.devices[0].modules.insert("9/9".into(), "Card-2S".into());
        back.devices[0].modules.insert("0/0".into(), "nope".into());
        back.sanitize();
        assert_eq!(back.devices[0].modules.len(), 1);
    }
}
