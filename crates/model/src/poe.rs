//! Power over Ethernet (IEEE 802.3af/at): PoE switches power the IP phones and lightweight
//! access points cabled to them.
//!
//! A powered device (an IP phone, a lightweight AP) has no power adapter plugged in when it is
//! placed: it boots only when a PoE switch port powers it (`power inline auto`, the default on
//! a PoE switch), or when its adapter is plugged in from the Physical tab. A plain switch gives
//! no power and leaves it dark, and says why. Each PoE switch has a budget: ports are powered in
//! port order while the budget lasts. `power inline never` keeps a port from powering anything.

use netlabcraft_core::DeviceId;

use crate::{Device, DeviceKind, End, Topology, models};

/// What a powered device draws (milliwatts): an IP phone 6.3 W, a lightweight AP 15.4 W (the
/// 802.3af class 0 maximum).
pub fn draw_mw(d: &Device) -> Option<u32> {
    if !needs_power(d.catalogue_model()) {
        return None;
    }
    Some(if d.kind == DeviceKind::Phone { 6_300 } else { 15_400 })
}

/// Whether a model is a powered device that can be powered over its network port.
pub fn needs_power(m: &models::Model) -> bool {
    matches!(m.id, "IP-Phone" | "LAP")
}

/// A PoE switch's power budget (milliwatts); `None`: the model gives no power.
pub fn budget_mw(m: &models::Model) -> Option<u32> {
    match m.id {
        "Switch-8-PoE" => Some(65_000),
        "Switch-24-PoE" => Some(370_000),
        _ => None,
    }
}

/// How a device is powered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Power {
    /// Not a powered device: its own supply.
    Mains,
    /// A powered device with its adapter plugged in.
    Adapter,
    /// Powered over Ethernet by this switch port, drawing this much.
    Inline { switch: DeviceId, port: usize, mw: u32 },
    /// No power: why.
    Dark(String),
}

/// One PoE port as `show power inline` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortPower {
    pub port: usize,
    /// `power inline never`.
    pub never: bool,
    /// The powered device on it and what it draws, when powered.
    pub device: Option<(DeviceId, u32)>,
    /// A powered device is cabled but the budget ran out.
    pub denied: bool,
}

impl Topology {
    /// The ports of a PoE switch and what each powers; empty for a switch that gives no power.
    pub fn power_inline(&self, switch: DeviceId) -> Vec<PortPower> {
        let Some(sw) = self.device(switch) else { return Vec::new() };
        let Some(budget) = budget_mw(sw.catalogue_model()) else { return Vec::new() };
        let mut used = 0u32;
        let mut out = Vec::new();
        for (i, p) in sw.ports.iter().enumerate() {
            let never = p.poe_never;
            let mut row = PortPower { port: i, never, device: None, denied: false };
            let far = self.peer(End { device: switch, port: i }).filter(|q| q.port == 0).and_then(|q| self.device(q.device));
            // A device on its adapter draws nothing from the port.
            if let Some(pd) = far.filter(|pd| pd.adapter == Some(false))
                && let Some(mw) = draw_mw(pd)
                && !never
                && !p.shutdown
                && !sw.powered_off
            {
                if used.saturating_add(mw) <= budget {
                    used = used.saturating_add(mw);
                    row.device = Some((pd.id, mw));
                } else {
                    row.denied = true;
                }
            }
            out.push(row);
        }
        out
    }

    /// How a device is powered (and, when it is dark, why).
    pub fn power(&self, id: DeviceId) -> Power {
        let Some(d) = self.device(id) else { return Power::Dark("no such device".into()) };
        if d.adapter != Some(false) || draw_mw(d).is_none() {
            return if d.adapter == Some(true) { Power::Adapter } else { Power::Mains };
        }
        let Some(q) = self.peer(End { device: id, port: 0 }) else {
            return Power::Dark("no power: plug in its power adapter (Physical tab) or cable it to a PoE switch".into());
        };
        let Some(sw) = self.device(q.device) else { return Power::Dark("no power".into()) };
        let port = sw.ports.get(q.port).map_or_else(String::new, |p| crate::ifname::short(&p.name));
        if budget_mw(sw.catalogue_model()).is_none() {
            return Power::Dark(format!(
                "no power: {} is a {}, which gives no power over Ethernet; use a PoE switch or plug in the power adapter",
                sw.name,
                sw.catalogue_model().label
            ));
        }
        if sw.powered_off {
            return Power::Dark(format!("no power: {} is switched off", sw.name));
        }
        let rows = self.power_inline(sw.id);
        match rows.iter().find(|r| r.port == q.port) {
            Some(PortPower { device: Some((_, mw)), .. }) => Power::Inline { switch: sw.id, port: q.port, mw: *mw },
            Some(r) if r.never => Power::Dark(format!("no power: {} {port} has `power inline never`", sw.name)),
            Some(r) if r.denied => Power::Dark(format!("no power: {}'s power budget is used up", sw.name)),
            _ => Power::Dark(format!("no power: {} {port} is shut down", sw.name)),
        }
    }

    /// Why a device is dark for lack of power, if it is.
    pub fn power_problem(&self, id: DeviceId) -> Option<String> {
        // Fast path: only powered devices without their adapter can be dark.
        if self.device(id).is_none_or(|d| d.adapter != Some(false)) {
            return None;
        }
        match self.power(id) {
            Power::Dark(why) => Some(why),
            _ => None,
        }
    }

    /// Whether a device is switched off or has no power.
    pub fn is_dark(&self, id: DeviceId) -> bool {
        self.device(id).is_some_and(|d| d.powered_off) || self.power_problem(id).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lab(switch: &str) -> (Topology, DeviceId, DeviceId) {
        let mut t = Topology::new();
        let s = t.add_model(models::find(switch).unwrap(), None, 0.0, 0.0).unwrap();
        let ph = t.add_model(models::find("IP-Phone").unwrap(), None, 0.0, 0.0).unwrap();
        t.connect(End { device: ph, port: 0 }, End { device: s, port: 0 }).unwrap();
        (t, s, ph)
    }

    #[test]
    fn a_poe_port_powers_a_phone_and_a_plain_switch_leaves_it_dark() {
        let (mut t, s, ph) = lab("Switch-24-PoE");
        assert_eq!(t.device(ph).unwrap().adapter, Some(false), "placed without its adapter");
        assert_eq!(t.power(ph), Power::Inline { switch: s, port: 0, mw: 6_300 });
        assert!(!t.is_dark(ph));
        t.device_mut(s).unwrap().ports[0].poe_never = true;
        assert!(t.power_problem(ph).unwrap().contains("power inline never"));
        t.device_mut(ph).unwrap().adapter = Some(true);
        assert_eq!(t.power(ph), Power::Adapter);
        let (t, _, ph) = lab("Switch-24");
        let why = t.power_problem(ph).unwrap();
        assert!(why.contains("gives no power over Ethernet"), "{why}");
        // Not a powered device: never dark for lack of power.
        let pc = t.devices.iter().find(|d| d.kind == DeviceKind::Switch).unwrap().id;
        assert_eq!(t.power(pc), Power::Mains);
    }

    #[test]
    fn the_budget_runs_out_in_port_order() {
        let mut t = Topology::new();
        let s = t.add_model(models::find("Switch-8-PoE").unwrap(), None, 0.0, 0.0).unwrap();
        let mut aps = Vec::new();
        for i in 0..5 {
            let ap = t.add_model(models::find("LAP").unwrap(), None, 0.0, 0.0).unwrap();
            t.connect(End { device: ap, port: 0 }, End { device: s, port: i }).unwrap();
            aps.push(ap);
        }
        // 65 W: four lightweight APs at 15.4 W, not five.
        let rows = t.power_inline(s);
        assert_eq!(rows.iter().filter(|r| r.device.is_some()).count(), 4);
        assert!(rows[4].denied);
        assert!(t.power_problem(aps[4]).unwrap().contains("budget"));
        assert!(t.power_problem(aps[0]).is_none());
    }
}
