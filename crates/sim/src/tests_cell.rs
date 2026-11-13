use super::*;
use netlabcraft_model::models;

fn ip(s: &str) -> Ipv4Addr {
    s.parse().unwrap()
}

struct Lab {
    t: Topology,
    phone: DeviceId,
    tower: DeviceId,
    core: DeviceId,
    server: DeviceId,
}

/// A smartphone near a tower; the tower's backhaul goes to a central office server (pool
/// 10.64.0.0/24), whose other port reaches a web server in the lab.
fn lab() -> Lab {
    let mut t = Topology::new();
    let tower = t.add_model(models::find("Cell Tower").unwrap(), None, 100.0, 0.0).unwrap();
    let core = t.add_model(models::find("Central Office Server").unwrap(), None, 300.0, 0.0).unwrap();
    let phone = t.add_model(models::find("Smartphone").unwrap(), None, 0.0, 0.0).unwrap();
    let server = t.add_device(DeviceKind::Server, None, 500.0, 0.0).unwrap();
    t.connect(End { device: tower, port: 0 }, End { device: core, port: 0 }).unwrap();
    t.connect(End { device: core, port: 1 }, End { device: server, port: 0 }).unwrap();
    {
        let c = t.device_mut(core).unwrap();
        for (i, a) in ["10.64.0.1/24", "192.168.50.1/24"].iter().enumerate() {
            c.ports[i].shutdown = false;
            c.ports[i].ip = Some(a.parse().unwrap());
        }
        c.cell.pool = Some("10.64.0.0/24".parse().unwrap());
    }
    {
        let s = t.device_mut(server).unwrap();
        s.ports[0].ip = Some("192.168.50.10/24".parse().unwrap());
        s.gateway = Some(ip("192.168.50.1"));
    }
    t.cell_refresh();
    Lab { t, phone, tower, core, server }
}

fn marks(probes: &[Probe]) -> String {
    probes.iter().map(|p| p.mark()).collect()
}

#[test]
fn a_phone_attaches_through_a_tower_and_pings_the_lab_over_the_cellular_link() {
    let l = lab();
    let mut sim = Sim::new();
    sim.topology_changed(&l.t);
    sim.settle(&l.t);
    // The attach exchange went through the event list, as real frames: request, then accept.
    let cell: Vec<_> = sim.trace().iter().filter(|e| e.protocol == Protocol::Cell).collect();
    assert!(cell.iter().any(|e| e.summary.starts_with("Cell attach request")), "{cell:#?}");
    assert!(cell.iter().any(|e| e.summary.contains("Cell attach accept: address 10.64.0.2/24, gateway 10.64.0.1")), "{cell:#?}");
    assert!(cell.iter().all(|e| e.dropped.is_none()), "{cell:#?}");
    for e in &cell {
        assert!(netlabcraft_wire::Frame::decode(&e.frame).is_ok());
    }
    let cp = l.t.device(l.phone).unwrap().cell_port().unwrap();
    assert_eq!(
        sim.cell_session(End { device: l.phone, port: cp }),
        Some(&cell::CellSession::Attached { address: ip("10.64.0.2"), prefix: 24, gateway: ip("10.64.0.1") })
    );
    // Data: ARP, then ICMP, across the air, the tower, the backhaul and the server's router.
    let r = sim.ping(&l.t, l.phone, ip("192.168.50.10"), PingOptions::default()).unwrap();
    assert!(marks(&r).ends_with("!!!!"), "{}", marks(&r));
    // And the answer's way back is the same cable of the lab: the server reaches the phone only
    // through a route the lab provides (here, none: its gateway is the central office).
    let back = sim.ping(&l.t, l.server, ip("10.64.0.2"), PingOptions::default()).unwrap();
    assert!(marks(&back).ends_with("!!!!"), "{}", marks(&back));
    let _ = l.tower;
    let _ = l.core;
}

#[test]
fn a_refused_attach_shows_the_reject_and_leaves_the_phone_without_an_address() {
    let mut l = lab();
    l.t.device_mut(l.core).unwrap().cell.plan_off = true;
    l.t.cell_refresh();
    let mut sim = Sim::new();
    sim.topology_changed(&l.t);
    sim.settle(&l.t);
    assert!(sim.trace().iter().any(|e| e.protocol == Protocol::Cell && e.summary.contains("Cell attach reject (code 2): the data plan is off")));
    let cp = l.t.device(l.phone).unwrap().cell_port().unwrap();
    assert_eq!(sim.cell_session(End { device: l.phone, port: cp }), Some(&cell::CellSession::Rejected("the data plan is off".into())));
    let r = sim.ping(&l.t, l.phone, ip("192.168.50.10"), PingOptions::default());
    assert!(r.is_err() || !marks(&r.unwrap()).contains('!'));
}

#[test]
fn leaving_the_coverage_drops_the_link_and_the_phone_attaches_again_on_return() {
    let mut l = lab();
    let mut sim = Sim::new();
    sim.topology_changed(&l.t);
    assert!(marks(&sim.ping(&l.t, l.phone, ip("192.168.50.10"), PingOptions::default()).unwrap()).contains('!'));
    // Out of range: no tower, no address, the echo goes nowhere.
    l.t.device_mut(l.phone).unwrap().x = 9000.0;
    l.t.cell_refresh();
    sim.topology_changed(&l.t);
    let cp = l.t.device(l.phone).unwrap().cell_port().unwrap();
    assert!(l.t.device(l.phone).unwrap().ports[cp].ip.is_none());
    assert!(sim.cell_session(End { device: l.phone, port: cp }).is_none());
    let r = sim.ping(&l.t, l.phone, ip("192.168.50.10"), PingOptions::default());
    assert!(r.is_err() || !marks(&r.unwrap()).contains('!'));
    // Back in range: it attaches again.
    l.t.device_mut(l.phone).unwrap().x = 0.0;
    l.t.cell_refresh();
    sim.topology_changed(&l.t);
    assert!(marks(&sim.ping(&l.t, l.phone, ip("192.168.50.10"), PingOptions::default()).unwrap()).contains('!'));
    let _ = l.tower;
}

#[test]
fn a_towers_sectors_do_not_see_each_others_frames() {
    let mut l = lab();
    let second = l.t.add_model(models::find("Smartphone").unwrap(), None, 20.0, 0.0).unwrap();
    l.t.cell_refresh();
    let mut sim = Sim::new();
    sim.topology_changed(&l.t);
    sim.settle(&l.t);
    sim.clear_trace();
    assert!(marks(&sim.ping(&l.t, l.phone, ip("192.168.50.10"), PingOptions::default()).unwrap()).contains('!'));
    let cp = |id| l.t.device(id).unwrap().cell_port().unwrap();
    // Nothing the first phone's pings did was carried to the second phone's radio.
    let second_end = End { device: second, port: cp(second) };
    assert!(!sim.trace().iter().any(|e| e.to == second_end && e.protocol == Protocol::Icmp), "ICMP leaked to another sector");
    assert_eq!(l.t.core_clients(l.core).len(), 2);
}

#[test]
fn a_phone_on_wifi_uses_wifi_and_falls_back_to_cellular_when_wifi_is_gone() {
    let mut l = lab();
    let ap = l.t.add_model(models::find("Access Point").unwrap(), None, 10.0, 10.0).unwrap();
    let local = l.t.add_device(DeviceKind::Server, None, 30.0, 10.0).unwrap();
    l.t.connect(End { device: ap, port: 0 }, End { device: local, port: 0 }).unwrap();
    let wifi = || Some(netlabcraft_model::Wifi { ssid: "home".into(), key: None, channel: 6, login: None });
    l.t.device_mut(ap).unwrap().ports[1].wifi = wifi();
    l.t.device_mut(local).unwrap().ports[0].ip = Some("192.168.9.10/24".parse().unwrap());
    {
        let p = l.t.device_mut(l.phone).unwrap();
        p.ports[0].wifi = wifi();
        p.ports[0].ip = Some("192.168.9.5/24".parse().unwrap());
    }
    l.t.cell_refresh();
    let mut sim = Sim::new();
    sim.topology_changed(&l.t);
    // Wi-Fi is up: the local server answers over the air, and cellular is not used.
    assert!(marks(&sim.ping(&l.t, l.phone, ip("192.168.9.10"), PingOptions::default()).unwrap()).contains('!'));
    assert!(sim.trace().iter().any(|e| e.air.is_some()));
    sim.clear_trace();
    let over_cell = sim.ping(&l.t, l.phone, ip("192.168.50.10"), PingOptions::default());
    assert!(over_cell.is_err() || !marks(&over_cell.unwrap()).contains('!'), "Wi-Fi is preferred, so the lab server is not reached over cellular");
    // Wi-Fi gone (no SSID): the cellular link carries the traffic.
    l.t.device_mut(l.phone).unwrap().ports[0].wifi = None;
    l.t.cell_refresh();
    sim.topology_changed(&l.t);
    assert!(marks(&sim.ping(&l.t, l.phone, ip("192.168.50.10"), PingOptions::default()).unwrap()).contains('!'));
}
