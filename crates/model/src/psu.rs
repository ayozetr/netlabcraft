//! Power supplies as parts: routers and switches have one or two bays for a power supply unit
//! at the back. A device leaves the factory with one supply fitted (bay 1); a second one, in a
//! model with two bays, is a spare that keeps it running when the first is taken out. Taking
//! out the last supply cuts the power: the device switches off as if its power switch were
//! pressed (the running configuration is gone), and it can't be switched on until a supply is
//! fitted again.

use crate::{Device, DeviceKind, models};

/// How many power-supply bays a model has (0: none, it is powered another way).
pub fn bays(model: &models::Model) -> usize {
    match model.id {
        "Router-2" | "Router-WAN" | "Router-M2" | "Firewall-5" | "Switch-8" | "Switch-8-PoE" | "WLC" => 1,
        "AccessPoint" | "LAP" | "Home-Router" | "Home-Gateway" => 0,
        _ if matches!(model.kind, DeviceKind::Router | DeviceKind::Switch) => 2,
        _ => 0,
    }
}

impl Device {
    /// Which bays hold a supply, bay 1 first (empty for a model without bays).
    pub fn psus(&self) -> Vec<bool> {
        let n = bays(self.catalogue_model());
        match &self.psu {
            Some(v) => (0..n).map(|i| v.get(i).copied().unwrap_or(false)).collect(),
            None => (0..n).map(|i| i == 0).collect(),
        }
    }

    /// Whether the device has a bay for a supply and none is fitted.
    pub fn no_psu(&self) -> bool {
        let p = self.psus();
        !p.is_empty() && p.iter().all(|f| !f)
    }

    /// Keeps a file's supplies to the model's bays; with none fitted the device is off.
    pub(crate) fn sanitize_psu(&mut self) {
        let n = bays(self.catalogue_model());
        if n == 0 {
            self.psu = None;
            return;
        }
        if let Some(v) = &mut self.psu {
            v.resize(n, false);
        }
        if self.no_psu() {
            self.powered_off = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Topology;

    #[test]
    fn devices_leave_the_factory_with_one_supply_and_files_are_trimmed() {
        let mut t = Topology::new();
        let r = t.add_model(models::find("Router-3").unwrap(), None, 0.0, 0.0).unwrap();
        let s = t.add_model(models::find("Switch-8").unwrap(), None, 0.0, 0.0).unwrap();
        let pc = t.add_model(models::find("PC").unwrap(), None, 0.0, 0.0).unwrap();
        assert_eq!(t.device(r).unwrap().psus(), [true, false]);
        assert_eq!(t.device(s).unwrap().psus(), [true]);
        assert!(t.device(pc).unwrap().psus().is_empty() && !t.device(pc).unwrap().no_psu());
        let d = t.device_mut(r).unwrap();
        d.psu = Some(vec![false, false, true, true]);
        t.device_mut(pc).unwrap().psu = Some(vec![true]);
        t.sanitize();
        let d = t.device(r).unwrap();
        assert_eq!(d.psu.as_deref(), Some(&[false, false][..]));
        assert!(d.no_psu() && d.powered_off, "no supply: off");
        assert!(t.device(pc).unwrap().psu.is_none());
    }
}
