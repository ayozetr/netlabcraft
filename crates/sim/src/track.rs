//! Object tracking: what a `track N …` object says about the lab right now, and the settled
//! state it reports after its delays. Static routes and first-hop redundancy groups read the
//! settled state ([`is_up`]); [`tick`] moves the settled state along as simulated time passes.

use std::cell::Cell;

use netlabcraft_core::DeviceId;
use netlabcraft_model::track::{Kind, Reported, SlaOutcome};
use netlabcraft_model::{Device, StaticRoute, Topology};

/// How deep a list of lists is followed before it counts as down (a list that names itself).
const MAX_DEPTH: u8 = 6;
/// How many times [`tick`] looks again while states keep changing (objects built on objects).
const PASSES: usize = 4;

thread_local! {
    /// Set while an object that watches a route reads the routing table: tracked static routes
    /// count as present then, so an object cannot wait for itself.
    static IN_ROUTE_CHECK: Cell<bool> = const { Cell::new(false) };
}

/// Clears [`IN_ROUTE_CHECK`] when the check ends, even on an unwind.
struct RouteCheck(bool);

impl RouteCheck {
    fn enter() -> RouteCheck {
        RouteCheck(IN_ROUTE_CHECK.with(|c| c.replace(true)))
    }
}

impl Drop for RouteCheck {
    fn drop(&mut self) {
        let before = self.0;
        IN_ROUTE_CHECK.with(|c| c.set(before));
    }
}

/// Whether a static route is in the table as far as tracking goes: it follows no object, or the
/// object is up.
pub fn route_usable(t: &Topology, d: &Device, r: &StaticRoute) -> bool {
    match d.track.follows(&r.net, &r.via) {
        None => true,
        Some(id) => IN_ROUTE_CHECK.with(Cell::get) || is_up(t, d, id),
    }
}

/// The state of an object as the rest of the simulator sees it: the settled one, or the live one
/// before the simulator has settled anything.
pub fn is_up(t: &Topology, d: &Device, id: u32) -> bool {
    settled(t, d, id, 0)
}

fn settled(t: &Topology, d: &Device, id: u32, depth: u8) -> bool {
    match d.track.reported.get(&id) {
        Some(r) => r.up,
        None => live(t, d, id, depth),
    }
}

/// The state of an object right now, before any delay.
pub fn live(t: &Topology, d: &Device, id: u32, depth: u8) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    let Some(o) = d.track.objects.get(&id) else { return false };
    match &o.kind {
        Kind::Interface { name, ip_routing } => {
            let up = crate::hsrp::tracked_up(t, d, name);
            if *ip_routing { up && d.routes_ip() && crate::up_ifaces(t, d).iter().any(|i| i.name.eq_ignore_ascii_case(name)) } else { up }
        }
        Kind::Route { net } => {
            let _guard = RouteCheck::enter();
            crate::routing_table(t, d).iter().any(|r| r.net == *net)
        }
        Kind::Sla { op, state } => match d.track.sla_outcome.get(op) {
            Some(SlaOutcome::Ok) => true,
            Some(SlaOutcome::OverThreshold) => !*state,
            _ => false,
        },
        Kind::List { and, members } => {
            let mut states = members.iter().map(|m| settled(t, d, m.id, depth.saturating_add(1)) != m.not);
            if *and { !members.is_empty() && states.all(|s| s) } else { states.any(|s| s) }
        }
    }
}

/// Moves every object's settled state to what the lab says now, after its delay. Returns whether
/// anything changed, so the caller refreshes what depends on it.
pub fn tick(t: &mut Topology, now_us: u64) -> bool {
    let ids: Vec<DeviceId> = t.devices.iter().filter(|d| !d.track.objects.is_empty()).map(|d| d.id).collect();
    let mut any = false;
    for _ in 0..PASSES {
        let mut round = false;
        for &dev in &ids {
            let Some(d) = t.device(dev) else { continue };
            let states: Vec<(u32, bool)> = d.track.objects.keys().map(|&id| (id, live(t, d, id, 0))).collect();
            let Some(dm) = t.device_mut(dev) else { continue };
            for (id, up) in states {
                let Some(o) = dm.track.objects.get(&id) else { continue };
                let delay = u64::from(if up { o.delay_up } else { o.delay_down }).saturating_mul(1_000_000);
                match dm.track.reported.get(&id).copied() {
                    None => {
                        dm.track.reported.insert(id, Reported { up, since_us: now_us, changes: 0 });
                        round = true;
                    }
                    Some(r) if r.up == up => {
                        dm.track.pending.remove(&id);
                    }
                    Some(r) => {
                        let started = match dm.track.pending.get(&id) {
                            Some((s, at)) if *s == up => *at,
                            _ => {
                                dm.track.pending.insert(id, (up, now_us));
                                now_us
                            }
                        };
                        if now_us.saturating_sub(started) >= delay {
                            dm.track.reported.insert(id, Reported { up, since_us: now_us, changes: r.changes.saturating_add(1) });
                            dm.track.pending.remove(&id);
                            round = true;
                        }
                    }
                }
            }
        }
        any |= round;
        if !round {
            break;
        }
    }
    any
}

#[cfg(test)]
mod tests {
    use super::*;
    use netlabcraft_core::Ipv4Net;
    use netlabcraft_model::track::{Member, Object};
    use netlabcraft_model::{DeviceKind, End, NextHop};

    fn net(s: &str) -> Ipv4Net {
        s.parse().unwrap()
    }

    /// Two routers on a link; R1 has a default route that follows an object watching its port.
    fn lab() -> (Topology, DeviceId, DeviceId) {
        let mut t = Topology::new();
        let r1 = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let r2 = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        t.connect(End { device: r1, port: 0 }, End { device: r2, port: 0 }).unwrap();
        for (d, ip) in [(r1, "10.0.0.1/30"), (r2, "10.0.0.2/30")] {
            let dev = t.device_mut(d).unwrap();
            dev.ports[0].ip = Some(net(ip));
            dev.ports[0].shutdown = false;
        }
        (t, r1, r2)
    }

    #[test]
    fn a_route_that_follows_a_down_interface_leaves_the_table() {
        let (mut t, r1, _) = lab();
        let d = t.device_mut(r1).unwrap();
        let via = NextHop::Address("10.0.0.2".parse().unwrap());
        d.routes.push(StaticRoute { net: net("0.0.0.0/0"), via: via.clone(), distance: None, tag: 0 });
        let name = d.ports[1].name.clone();
        d.track.objects.insert(1, Object::new(Kind::Interface { name: name.clone(), ip_routing: false }));
        d.track.route_track.insert(netlabcraft_model::track::route_key(&net("0.0.0.0/0"), &via), 1);
        // Port 1 has no cable: the object is down, so the default route is out of the table.
        let d = t.device(r1).unwrap();
        assert!(!is_up(&t, d, 1));
        assert!(crate::routing_table(&t, d).iter().all(|r| r.code != 'S'));
        // Without the binding the route is back.
        let d = t.device_mut(r1).unwrap();
        d.track.route_track.clear();
        let d = t.device(r1).unwrap();
        assert!(crate::routing_table(&t, d).iter().any(|r| r.code == 'S'));
    }

    #[test]
    fn a_delay_holds_back_a_change_until_it_has_lasted() {
        let (mut t, r1, _) = lab();
        let d = t.device_mut(r1).unwrap();
        d.track
            .objects
            .insert(1, Object { kind: Kind::Interface { name: "GigabitEthernet0/0".into(), ip_routing: false }, delay_up: 0, delay_down: 10 });
        assert!(tick(&mut t, 0));
        assert!(t.device(r1).unwrap().track.reported[&1].up);
        t.device_mut(r1).unwrap().ports[0].shutdown = true;
        tick(&mut t, 1_000_000);
        assert!(t.device(r1).unwrap().track.reported[&1].up, "the delay has not passed");
        tick(&mut t, 12_000_000);
        let r = t.device(r1).unwrap().track.reported[&1];
        assert!(!r.up && r.changes == 1);
    }

    #[test]
    fn lists_combine_their_members_and_a_list_of_itself_is_down() {
        let (mut t, r1, _) = lab();
        let d = t.device_mut(r1).unwrap();
        d.track.objects.insert(1, Object::new(Kind::Sla { op: 1, state: false }));
        d.track.objects.insert(2, Object::new(Kind::Sla { op: 2, state: false }));
        d.track.sla_outcome.insert(1, SlaOutcome::Ok);
        d.track.sla_outcome.insert(2, SlaOutcome::Failed);
        let m = |id, not| Member { id, not };
        d.track.objects.insert(3, Object::new(Kind::List { and: true, members: vec![m(1, false), m(2, false)] }));
        d.track.objects.insert(4, Object::new(Kind::List { and: false, members: vec![m(1, false), m(2, false)] }));
        d.track.objects.insert(5, Object::new(Kind::List { and: true, members: vec![m(1, false), m(2, true)] }));
        d.track.objects.insert(6, Object::new(Kind::List { and: true, members: vec![m(6, false)] }));
        let d = t.device(r1).unwrap();
        assert!(!is_up(&t, d, 3) && is_up(&t, d, 4) && is_up(&t, d, 5) && !is_up(&t, d, 6));
    }

    #[test]
    fn a_route_object_sees_the_table_and_cannot_wait_for_itself() {
        let (mut t, r1, _) = lab();
        let d = t.device_mut(r1).unwrap();
        let via = NextHop::Address("10.0.0.2".parse().unwrap());
        d.routes.push(StaticRoute { net: net("0.0.0.0/0"), via: via.clone(), distance: None, tag: 0 });
        d.track.objects.insert(1, Object::new(Kind::Route { net: net("0.0.0.0/0") }));
        d.track.route_track.insert(netlabcraft_model::track::route_key(&net("0.0.0.0/0"), &via), 1);
        d.track.objects.insert(2, Object::new(Kind::Route { net: net("172.16.0.0/16") }));
        let d = t.device(r1).unwrap();
        assert!(is_up(&t, d, 1), "a route that follows its own object is not removed by it");
        assert!(!is_up(&t, d, 2));
    }
}
