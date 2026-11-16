//! The provider cloud: a WAN device whose ports are joined in pairs (`Coaxial0` to `Phone0`,
//! `Ethernet0` to `Ethernet1`…): a frame arriving on a port leaves only by the port it is
//! mapped to. The modems are plain two-port bridges (a phone line or a coaxial cable to an
//! Ethernet port), simulated as repeaters.
//!
//! A telephone cloud (`PSTN`) is the same device with eight phone ports, no pairs at first and a
//! number per port: a call is a pair made for as long as it lasts (see `engine::dialup`).

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

/// Longest telephone number.
pub const MAX_NUMBER_DIGITS: usize = 15;

/// A telephone number as digits only: spaces, dashes, dots, parentheses and a leading `+` are
/// formatting and go. `None` when nothing is left, or something else is in it.
pub fn normalize_number(text: &str) -> Option<String> {
    let mut digits = String::new();
    for (i, ch) in text.trim().chars().enumerate() {
        match ch {
            '0'..='9' => digits.push(ch),
            ' ' | '-' | '.' | '(' | ')' => {}
            '+' if i == 0 => {}
            _ => return None,
        }
    }
    (!digits.is_empty() && digits.len() <= MAX_NUMBER_DIGITS).then_some(digits)
}

impl Device {
    /// The port of this telephone cloud that has `number`.
    pub fn port_of_number(&self, number: &str) -> Option<usize> {
        let name = self.phone_numbers.iter().find_map(|(p, n)| (n == number).then_some(p))?;
        self.ports.iter().position(|p| &p.name == name)
    }

    /// Drops numbers of ports the device doesn't have, repeated numbers, bad numbers, and numbers
    /// on devices that aren't telephone clouds (hostile files).
    pub(crate) fn sanitize_phone_numbers(&mut self) {
        if !self.catalogue_model().pstn() {
            self.phone_numbers.clear();
            return;
        }
        let names: Vec<String> = self.ports.iter().map(|p| p.name.clone()).collect();
        let mut seen = std::collections::BTreeSet::new();
        self.phone_numbers.retain(|port, number| {
            names.contains(port) && normalize_number(number).as_deref() == Some(number.as_str()) && seen.insert(number.clone())
        });
    }
}

impl Topology {
    /// Gives a phone port of a telephone cloud a number (`None` takes it away). Numbers are
    /// digits, at most [`MAX_NUMBER_DIGITS`], and one port has each.
    pub fn cloud_number(&mut self, id: DeviceId, port: &str, number: Option<&str>) -> Result<()> {
        let d = self.device(id).ok_or_else(|| ModelError::NoDevice(id.to_string()))?;
        if !d.catalogue_model().pstn() {
            return Err(ModelError::Invalid(format!("{} is not a telephone cloud", d.name)));
        }
        let p = d.port_index(port).ok_or_else(|| ModelError::Invalid(format!("{} has no port {port}", d.name)))?;
        let name = d.ports.get(p).map(|x| x.name.clone()).unwrap_or_default();
        let bad = |text: &str| ModelError::Invalid(format!("`{text}` is not a telephone number: use 1 to {MAX_NUMBER_DIGITS} digits"));
        let digits = match number {
            None => None,
            Some(text) => Some(normalize_number(text).ok_or_else(|| bad(text))?),
        };
        if let Some(n) = &digits
            && d.phone_numbers.iter().any(|(other, x)| x == n && *other != name)
        {
            return Err(ModelError::Invalid(format!("{n} is already the number of another port")));
        }
        if let Some(d) = self.device_mut(id) {
            match digits {
                Some(n) => {
                    d.phone_numbers.insert(name, n);
                }
                None => {
                    d.phone_numbers.remove(&name);
                }
            }
        }
        Ok(())
    }

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

    #[test]
    fn a_telephone_cloud_has_numbers_one_per_port_and_hostile_ones_go() {
        let mut t = Topology::new();
        let c = t.add_model(models::find("Telephone Cloud").unwrap(), None, 0.0, 0.0).unwrap();
        assert_eq!(t.device(c).unwrap().name, "PSTN1");
        assert!(t.device(c).unwrap().bridges.is_empty(), "no pair at first");
        assert_eq!(super::normalize_number("+34 (922) 55-10.01").as_deref(), Some("34922551001"));
        assert!(super::normalize_number("").is_none() && super::normalize_number("12x").is_none());
        assert!(super::normalize_number("1234567890123456").is_none());
        t.cloud_number(c, "ph0", Some("555-1001")).unwrap();
        t.cloud_number(c, "Phone1", Some("5551002")).unwrap();
        assert!(t.cloud_number(c, "Phone1", Some("5551001")).is_err(), "a number belongs to one port");
        t.cloud_number(c, "Phone1", Some("5551002")).unwrap();
        assert_eq!(t.device(c).unwrap().port_of_number("5551001"), t.device(c).unwrap().port_index("Phone0"));
        assert!(t.cloud_number(c, "Phone9", Some("1")).is_err() && t.cloud_number(c, "Phone0", Some("abc")).is_err());
        t.cloud_map(c, "Phone0", "Phone1", true).unwrap();
        let mut back = crate::file::from_json(&crate::file::to_json(&t).unwrap()).unwrap();
        assert_eq!(back.device(c).unwrap().phone_numbers, t.device(c).unwrap().phone_numbers);
        back.device_mut(c).unwrap().phone_numbers.insert("Phone7".into(), "12".into());
        back.device_mut(c).unwrap().phone_numbers.insert("Phone6".into(), "12".into());
        back.device_mut(c).unwrap().phone_numbers.insert("Nope".into(), "99".into());
        back.device_mut(c).unwrap().phone_numbers.insert("Phone5".into(), "x1".into());
        let again = crate::file::from_json(&crate::file::to_json(&back).unwrap()).unwrap();
        let kept = &again.device(c).unwrap().phone_numbers;
        assert_eq!(kept.len(), 3, "{kept:?}");
        assert!(!kept.contains_key("Nope") && !kept.contains_key("Phone5"));
        t.cloud_number(c, "Phone0", None).unwrap();
        assert!(t.device(c).unwrap().port_of_number("5551001").is_none());
    }
}
