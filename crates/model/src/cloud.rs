//! The provider cloud: a WAN device whose ports are joined in pairs (`Coaxial0` to `Phone0`,
//! `Ethernet0` to `Ethernet1`…): a frame arriving on a port leaves only by the port it is
//! mapped to. The modems are plain two-port bridges (a phone line or a coaxial cable to an
//! Ethernet port), simulated as repeaters.

use crate::{Device, DeviceId, ModelError, Result, Topology};

/// Most pairs a cloud keeps.
pub const MAX_BRIDGES: usize = 64;

impl Device {
    /// The port a cloud joins `port` to.
    pub fn cloud_partner(&self, port: usize) -> Option<usize> {
        let name = &self.ports.get(port)?.name;
        let other = self.bridges.iter().find_map(|(a, b)| {
            if a == name {
                Some(b)
            } else if b == name {
                Some(a)
            } else {
                None
            }
        })?;
        self.ports.iter().position(|p| &p.name == other)
    }

    /// Drops pairs naming ports the device doesn't have, repeated ports, and pairs on devices
    /// that aren't clouds (hostile files).
    pub(crate) fn sanitize_bridges(&mut self) {
        if !self.catalogue_model().cloud() {
            self.bridges.clear();
            return;
        }
        let names: Vec<String> = self.ports.iter().map(|p| p.name.clone()).collect();
        let mut seen = std::collections::BTreeSet::new();
        self.bridges.truncate(MAX_BRIDGES);
        self.bridges.retain(|(a, b)| a != b && names.contains(a) && names.contains(b) && seen.insert(a.clone()) && seen.insert(b.clone()));
    }
}

impl Topology {
    /// Joins two ports of a cloud (`add`), or parts them. A port is in one pair at most.
    pub fn cloud_map(&mut self, id: DeviceId, a: &str, b: &str, add: bool) -> Result<()> {
        let d = self.device(id).ok_or_else(|| ModelError::NoDevice(id.to_string()))?;
        if !d.catalogue_model().cloud() {
            return Err(ModelError::Invalid(format!("{} is not a cloud", d.name)));
        }
        let pa = d.port_index(a).ok_or_else(|| ModelError::Invalid(format!("{} has no port {a}", d.name)))?;
        let pb = d.port_index(b).ok_or_else(|| ModelError::Invalid(format!("{} has no port {b}", d.name)))?;
        let (na, nb) = (d.ports.get(pa).map(|p| p.name.clone()).unwrap_or_default(), d.ports.get(pb).map(|p| p.name.clone()).unwrap_or_default());
        if pa == pb {
            return Err(ModelError::Invalid(format!("{na} can't be joined to itself")));
        }
        let pair = |x: &(String, String)| (x.0 == na && x.1 == nb) || (x.0 == nb && x.1 == na);
        if add {
            if let Some(p) = [pa, pb].into_iter().find(|p| d.cloud_partner(*p).is_some()) {
                let name = d.ports.get(p).map(|x| x.name.clone()).unwrap_or_default();
                return Err(ModelError::Invalid(format!("{name} is already joined to another port: part it first")));
            }
            if d.bridges.len() >= MAX_BRIDGES {
                return Err(ModelError::Invalid("too many port pairs".into()));
            }
        } else if !d.bridges.iter().any(pair) {
            return Err(ModelError::Invalid(format!("{na} and {nb} are not joined")));
        }
        if let Some(d) = self.device_mut(id) {
            if add {
                d.bridges.push((na, nb));
            } else {
                d.bridges.retain(|x| !pair(x));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{Topology, models};

    #[test]
    fn a_cloud_joins_ports_in_pairs_and_hostile_pairs_go() {
        let mut t = Topology::new();
        let c = t.add_model(models::find("Cloud").unwrap(), None, 0.0, 0.0).unwrap();
        let m = t.add_model(models::find("DSL Modem").unwrap(), None, 0.0, 0.0).unwrap();
        assert_eq!(t.device(c).unwrap().name, "Cloud1");
        assert_eq!(t.device(m).unwrap().name, "Modem1");
        let d = t.device(c).unwrap();
        let co0 = d.port_index("Coaxial0").unwrap();
        let ph0 = d.port_index("Phone0").unwrap();
        assert_eq!(d.cloud_partner(co0), Some(ph0));
        assert!(t.cloud_map(c, "co0", "e0", true).is_err(), "Coaxial0 is taken");
        t.cloud_map(c, "co0", "ph0", false).unwrap();
        t.cloud_map(c, "Coaxial0", "Ethernet0", true).unwrap_err();
        t.cloud_map(c, "e0", "e1", false).unwrap();
        t.cloud_map(c, "Coaxial0", "Ethernet0", true).unwrap();
        assert!(t.cloud_map(c, "e0", "e0", true).is_err());
        assert!(t.cloud_map(c, "s0/0/0", "e0", true).is_err());
        assert!(t.cloud_map(m, "ph0", "e1", true).is_err(), "a modem is not a cloud");
        let mut back = crate::file::from_json(&crate::file::to_json(&t).unwrap()).unwrap();
        assert_eq!(back.device(c).unwrap().bridges, t.device(c).unwrap().bridges);
        back.devices[0].bridges.push(("Nope0".into(), "Ethernet1".into()));
        back.devices[0].bridges.push(("Coaxial0".into(), "Ethernet1".into()));
        back.devices[1].bridges.push(("Phone0".into(), "Ethernet1".into()));
        back.sanitize();
        assert_eq!(back.devices[0].bridges, t.device(c).unwrap().bridges);
        assert!(back.devices[1].bridges.is_empty());
    }
}
