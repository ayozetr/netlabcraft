use super::*;
use netlabcraft_core::VlanSet;
use netlabcraft_model::{StaticRoute, SubIf};

fn net(s: &str) -> Ipv4Net {
    s.parse().unwrap()
}

fn ip(s: &str) -> Ipv4Addr {
    s.parse().unwrap()
}

fn add(t: &mut Topology, kind: DeviceKind) -> DeviceId {
    t.add_device(kind, None, 0.0, 0.0).unwrap()
}

fn cable(t: &mut Topology, a: DeviceId, pa: usize, b: DeviceId, pb: usize) {
    t.connect(End { device: a, port: pa }, End { device: b, port: pb }).unwrap();
}

fn host(t: &mut Topology, addr: &str, gw: Option<&str>) -> DeviceId {
    let h = add(t, DeviceKind::Pc);
    let d = t.device_mut(h).unwrap();
    d.ports[0].ip = Some(net(addr));
    d.gateway = gw.map(ip);
    h
}

fn router_if(t: &mut Topology, r: DeviceId, port: usize, addr: &str) {
    let p = &mut t.device_mut(r).unwrap().ports[port];
    p.shutdown = false;
    p.ip = Some(net(addr));
}

fn marks(probes: &[Probe]) -> String {
    probes.iter().map(|p| p.mark()).collect()
}

#[test]
fn hosts_on_one_switch_reach_each_other_and_the_switch_learns() {
    let mut t = Topology::new();
    let s = add(&mut t, DeviceKind::Switch);
    let a = host(&mut t, "192.168.1.10/24", None);
    let b = host(&mut t, "192.168.1.11/24", None);
    cable(&mut t, a, 0, s, 0);
    cable(&mut t, b, 0, s, 1);
    let mut sim = Sim::new();
    let r = sim.ping(&t, a, ip("192.168.1.11"), PingOptions::default()).unwrap();
    // Hosts hold the first packet while ARP resolves, so nothing is lost.
    assert_eq!(marks(&r), "!!!!!");
    let Probe::Reply { rtt_us, .. } = r[1] else { panic!() };
    assert_eq!(rtt_us, 4 * LINK_DELAY_US);
    let macs = &sim.state(s).unwrap().macs;
    assert_eq!(macs.len(), 2);
    assert!(macs.values().any(|e| e.port == 0) && macs.values().any(|e| e.port == 1));
    assert_eq!(sim.state(a).unwrap().arp.get(&ip("192.168.1.11")).map(|e| e.mac), Some(t.device(b).unwrap().ports[0].mac));
    // The event list holds the ARP exchange and the echoes, all delivered.
    assert!(sim.trace().iter().any(|e| e.protocol == Protocol::Arp));
    assert!(sim.trace().iter().filter(|e| e.protocol == Protocol::Icmp && e.dropped.is_none()).count() >= 10);
    let pcap = sim.pcap();
    assert!(pcap.len() > 24 + sim.trace().len() * 16);
}

#[test]
fn a_router_joins_two_subnets_and_drops_the_first_packet_while_it_arps() {
    let mut t = Topology::new();
    let r = add(&mut t, DeviceKind::Router);
    let a = host(&mut t, "10.0.1.10/24", Some("10.0.1.1"));
    let b = host(&mut t, "10.0.2.10/24", Some("10.0.2.1"));
    router_if(&mut t, r, 0, "10.0.1.1/24");
    router_if(&mut t, r, 1, "10.0.2.1/24");
    cable(&mut t, a, 0, r, 0);
    cable(&mut t, b, 0, r, 1);
    let mut sim = Sim::new();
    assert_eq!(marks(&sim.ping(&t, a, ip("10.0.2.10"), PingOptions::default()).unwrap()), ".!!!!");
    assert_eq!(marks(&sim.ping(&t, a, ip("10.0.2.10"), PingOptions::default()).unwrap()), "!!!!!");
    // The router answers for itself too.
    assert_eq!(marks(&sim.ping(&t, b, ip("10.0.1.1"), PingOptions::default()).unwrap()), "!!!!!");
    // Without a gateway, the far subnet is unreachable from the start.
    t.device_mut(b).unwrap().gateway = None;
    assert!(matches!(sim.ping(&t, b, ip("10.0.1.10"), PingOptions::default()), Err(SimError::NoSource(_))));
}

#[test]
fn routers_start_shut_down() {
    let mut t = Topology::new();
    let r = add(&mut t, DeviceKind::Router);
    let a = host(&mut t, "10.0.1.10/24", Some("10.0.1.1"));
    t.device_mut(r).unwrap().ports[0].ip = Some(net("10.0.1.1/24"));
    cable(&mut t, a, 0, r, 0);
    let mut sim = Sim::new();
    assert!(!port_up(&t, End { device: r, port: 0 }));
    assert_eq!(marks(&sim.ping(&t, a, ip("10.0.1.1"), PingOptions { count: 2, ..Default::default() }).unwrap()), "..");
    t.device_mut(r).unwrap().ports[0].shutdown = false;
    sim.topology_changed(&t);
    assert_eq!(marks(&sim.ping(&t, a, ip("10.0.1.1"), PingOptions { count: 2, ..Default::default() }).unwrap()), "!!");
}

#[test]
fn vlans_split_a_switch() {
    let mut t = Topology::new();
    let s = add(&mut t, DeviceKind::Switch);
    let a = host(&mut t, "10.0.0.1/24", None);
    let b = host(&mut t, "10.0.0.2/24", None);
    let c = host(&mut t, "10.0.0.3/24", None);
    cable(&mut t, a, 0, s, 0);
    cable(&mut t, b, 0, s, 1);
    cable(&mut t, c, 0, s, 2);
    {
        let sw = t.device_mut(s).unwrap();
        let v10 = VlanId::new(10).unwrap();
        sw.vlans.insert(v10, "Ten".into());
        sw.ports[0].switchport.access_vlan = v10;
        sw.ports[1].switchport.access_vlan = v10;
    }
    let mut sim = Sim::new();
    let opts = PingOptions { count: 2, ..Default::default() };
    assert_eq!(marks(&sim.ping(&t, a, ip("10.0.0.2"), opts).unwrap()), "!!");
    assert_eq!(marks(&sim.ping(&t, a, ip("10.0.0.3"), opts).unwrap()), "..");
    // An access VLAN missing from the database carries nothing.
    t.device_mut(s).unwrap().vlans.remove(&VlanId::new(10).unwrap());
    sim.reset();
    assert_eq!(marks(&sim.ping(&t, a, ip("10.0.0.2"), opts).unwrap()), "..");
    assert!(sim.trace().iter().any(|e| e.dropped.as_deref() == Some("VLAN 10 does not exist on this switch")));
}

#[test]
fn router_on_a_stick_routes_between_vlans_over_a_trunk() {
    let mut t = Topology::new();
    let r = add(&mut t, DeviceKind::Router);
    let s = add(&mut t, DeviceKind::Switch);
    let a = host(&mut t, "192.168.10.10/24", Some("192.168.10.1"));
    let b = host(&mut t, "192.168.20.10/24", Some("192.168.20.1"));
    cable(&mut t, r, 0, s, 24);
    cable(&mut t, a, 0, s, 0);
    cable(&mut t, b, 0, s, 1);
    {
        let sw = t.device_mut(s).unwrap();
        for (v, port) in [(10, 0), (20, 1)] {
            let v = VlanId::new(v).unwrap();
            sw.vlans.insert(v, Device::default_vlan_name(v));
            sw.ports[port].switchport.access_vlan = v;
        }
        sw.ports[24].switchport.mode = SwitchportMode::Trunk;
    }
    {
        let rt = t.device_mut(r).unwrap();
        rt.ports[0].shutdown = false;
        for (n, a) in [(10, "192.168.10.1/24"), (20, "192.168.20.1/24")] {
            rt.ports[0].subifs.push(SubIf {
                number: n,
                vlan: VlanId::new(n as u16),
                ip: Some(net(a)),
                shutdown: false,
                description: String::new(),
                helpers: Vec::new(),
                access: Default::default(),
                nat: None,
                standby: Vec::new(),
                ipv6: Default::default(),
            });
        }
    }
    let mut sim = Sim::new();
    assert_eq!(marks(&sim.ping(&t, a, ip("192.168.20.10"), PingOptions::default()).unwrap()), ".!!!!");
    // Frames between switch and router carry 802.1Q tags.
    let trunk = End { device: s, port: 24 };
    let tags: Vec<_> = sim.trace().iter().filter(|e| e.to == trunk || e.from == trunk).filter_map(|e| Frame::decode(&e.frame).ok()?.vlan).collect();
    assert!(tags.contains(&10) && tags.contains(&20));
    // Pruning VLAN 20 off the trunk cuts it.
    t.device_mut(s).unwrap().ports[24].switchport.allowed = VlanSet::parse_list("1-10").unwrap();
    sim.reset();
    assert_eq!(marks(&sim.ping(&t, a, ip("192.168.20.10"), PingOptions { count: 3, ..Default::default() }).unwrap()), "...");
}

/// PC1 — R1 — R2 — PC2, static routes both ways.
fn chain() -> (Topology, DeviceId, DeviceId, DeviceId, DeviceId) {
    let mut t = Topology::new();
    let r1 = add(&mut t, DeviceKind::Router);
    let r2 = add(&mut t, DeviceKind::Router);
    let a = host(&mut t, "10.1.0.10/24", Some("10.1.0.1"));
    let b = host(&mut t, "10.2.0.10/24", Some("10.2.0.1"));
    router_if(&mut t, r1, 0, "10.1.0.1/24");
    router_if(&mut t, r1, 1, "172.16.0.1/30");
    router_if(&mut t, r2, 1, "172.16.0.2/30");
    router_if(&mut t, r2, 0, "10.2.0.1/24");
    cable(&mut t, a, 0, r1, 0);
    cable(&mut t, r1, 1, r2, 1);
    cable(&mut t, b, 0, r2, 0);
    t.device_mut(r1).unwrap().routes.push(StaticRoute { net: net("10.2.0.0/24"), via: NextHop::Address(ip("172.16.0.2")) });
    t.device_mut(r2).unwrap().routes.push(StaticRoute { net: net("0.0.0.0/0"), via: NextHop::Interface("g0/1".into()) });
    (t, r1, r2, a, b)
}

#[test]
fn static_routes_and_traceroute() {
    let (t, r1, r2, a, _) = chain();
    let mut sim = Sim::new();
    // Warm the ARP caches, then trace.
    sim.ping(&t, a, ip("10.2.0.10"), PingOptions::default()).unwrap();
    assert_eq!(marks(&sim.ping(&t, a, ip("10.2.0.10"), PingOptions::default()).unwrap()), "!!!!!");
    let hops = sim.traceroute(&t, a, ip("10.2.0.10"), 30, 2_000_000).unwrap();
    let firsts: Vec<_> = hops
        .iter()
        .map(|h| match h.probes[0] {
            Probe::TtlExceeded { from, .. } | Probe::Reply { from, .. } => from.to_string(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(firsts, ["10.1.0.1", "172.16.0.2", "10.2.0.10"]);
    let table = routing_table(&t, t.device(r1).unwrap());
    let codes: String = table.iter().map(|r| r.code).collect();
    assert_eq!(codes, "CLSCL");
    assert!(table.iter().any(|r| r.code == 'S' && r.via == Some(ip("172.16.0.2"))));
    assert!(routing_table(&t, t.device(r2).unwrap()).iter().any(|r| r.code == 'S' && r.iface.as_deref() == Some("GigabitEthernet0/1")));
}

#[test]
fn unreachable_and_ttl_errors_come_back() {
    let (t, ..) = chain();
    let (_, _, _, a, _) = chain();
    let mut sim = Sim::new();
    // R1 has no route to 10.9.0.0: it answers "network unreachable".
    let r = sim.ping(&t, a, ip("10.9.0.1"), PingOptions { count: 2, ..Default::default() }).unwrap();
    assert_eq!(marks(&r), "UU");
    let r = sim.ping(&t, a, ip("10.2.0.10"), PingOptions { count: 3, ttl: Some(1), ..Default::default() }).unwrap();
    assert_eq!(marks(&r), "TTT");
}

#[test]
fn shutting_a_port_down_cuts_traffic_and_flushes_state() {
    let (mut t, r1, ..) = chain();
    let (_, _, _, a, _) = chain();
    let mut sim = Sim::new();
    sim.ping(&t, a, ip("10.2.0.10"), PingOptions::default()).unwrap();
    assert!(sim.state(r1).unwrap().arp.contains_key(&ip("172.16.0.2")));
    t.device_mut(r1).unwrap().ports[1].shutdown = true;
    sim.topology_changed(&t);
    assert!(!sim.state(r1).unwrap().arp.contains_key(&ip("172.16.0.2")));
    // R1 loses the connected route, so the static route via 172.16.0.2 goes with it.
    assert_eq!(marks(&sim.ping(&t, a, ip("10.2.0.10"), PingOptions { count: 2, ..Default::default() }).unwrap()), "UU");
}

#[test]
fn a_switching_loop_through_hubs_trips_the_storm_guard_instead_of_hanging() {
    // Hubs don't run spanning tree, so a hub loop really loops.
    let mut t = Topology::new();
    let s1 = add(&mut t, DeviceKind::Hub);
    let s2 = add(&mut t, DeviceKind::Hub);
    let a = host(&mut t, "10.0.0.1/24", None);
    cable(&mut t, s1, 0, s2, 0);
    cable(&mut t, s1, 1, s2, 1);
    cable(&mut t, a, 0, s1, 2);
    let mut sim = Sim::new();
    let r = sim.ping(&t, a, ip("10.0.0.99"), PingOptions { count: 1, ..Default::default() }).unwrap();
    assert_eq!(marks(&r), ".");
    assert!(sim.storm_detected());
    assert!(sim.trace().iter().any(|e| e.dropped.as_deref().is_some_and(|d| d.starts_with("loop"))));
    assert!(sim.trace().len() <= TRACE_CAP);
    sim.settle(&t);
}

#[test]
fn removing_a_cable_drops_frames_in_flight() {
    let mut t = Topology::new();
    let a = host(&mut t, "10.0.0.1/24", None);
    let b = host(&mut t, "10.0.0.2/24", None);
    cable(&mut t, a, 0, b, 0);
    let mut sim = Sim::new();
    let d = t.device(a).unwrap().clone();
    sim.send_ip(
        &t,
        &d,
        Ipv4 {
            id: 1,
            ttl: 64,
            protocol: PROTO_ICMP,
            src: ip("10.0.0.1"),
            dst: ip("10.0.0.2"),
            payload: Icmp::EchoRequest { id: 1, seq: 1, data: vec![] }.encode(),
        },
    )
    .unwrap();
    t.disconnect(End { device: a, port: 0 });
    sim.topology_changed(&t);
    sim.settle(&t);
    assert!(sim.trace().is_empty());
}

#[test]
fn hostile_frames_are_dropped_not_fatal() {
    let mut t = Topology::new();
    let s = add(&mut t, DeviceKind::Switch);
    let r = add(&mut t, DeviceKind::Router);
    let a = host(&mut t, "10.0.0.1/24", None);
    router_if(&mut t, r, 0, "10.0.0.2/24");
    cable(&mut t, a, 0, s, 0);
    cable(&mut t, r, 0, s, 1);
    let mut sim = Sim::new();
    let garbage: [&[u8]; 4] = [&[], &[0xff; 13], &[0xff; 20], &[0xff; 200]];
    for g in garbage {
        for to in [End { device: s, port: 0 }, End { device: r, port: 0 }, End { device: a, port: 0 }] {
            sim.queue.push(Reverse(Queued { at: 0, seq: 0, from: to, to, frame: g.to_vec(), hops: 0 }));
        }
    }
    sim.settle(&t);
    // (Spanning-tree BPDUs from the switch are taken in quietly; everything else was garbage.)
    assert!(sim.trace().iter().all(|e| e.dropped.is_some() || e.to.device == s || e.protocol == netlabcraft_wire::Protocol::Stp));
}

#[test]
fn a_hub_repeats_to_everyone_and_hosts_ignore_what_is_not_theirs() {
    let mut t = Topology::new();
    let h = add(&mut t, DeviceKind::Hub);
    let a = host(&mut t, "10.0.0.1/24", None);
    let b = host(&mut t, "10.0.0.2/24", None);
    let c = add(&mut t, DeviceKind::Laptop);
    t.device_mut(c).unwrap().ports[0].ip = Some(net("10.0.0.3/24"));
    for (i, d) in [a, b, c].into_iter().enumerate() {
        cable(&mut t, d, 0, h, i);
    }
    let mut sim = Sim::new();
    assert_eq!(marks(&sim.ping(&t, a, ip("10.0.0.2"), PingOptions { count: 2, ..Default::default() }).unwrap()), "!!");
    // The laptop saw the unicast echo too (a hub floods everything) and dropped it.
    assert!(sim.trace().iter().any(|e| e.to.device == c && e.protocol == Protocol::Icmp && e.dropped.is_some()));
}

#[test]
fn dhcp_hands_out_addresses_from_a_pool() {
    use netlabcraft_model::DhcpPool;
    let mut t = Topology::new();
    let r = add(&mut t, DeviceKind::Router);
    let s = add(&mut t, DeviceKind::Switch);
    let a = add(&mut t, DeviceKind::Pc);
    let b = add(&mut t, DeviceKind::Laptop);
    router_if(&mut t, r, 0, "192.168.1.1/24");
    {
        let rt = t.device_mut(r).unwrap();
        rt.dhcp_pools.push(DhcpPool {
            name: "LAN".into(),
            network: Some(net("192.168.1.0/24")),
            default_router: Some(ip("192.168.1.1")),
            dns: Some(ip("192.168.1.2")),
        });
        rt.dhcp_excluded.push((ip("192.168.1.1"), ip("192.168.1.10")));
    }
    cable(&mut t, r, 0, s, 0);
    cable(&mut t, a, 0, s, 1);
    cable(&mut t, b, 0, s, 2);
    let mut sim = Sim::new();
    let la = sim.dhcp_acquire(&t, a).unwrap();
    assert_eq!(la.address.to_string(), "192.168.1.11/24");
    assert_eq!(la.router, Some(ip("192.168.1.1")));
    assert_eq!(la.dns, Some(ip("192.168.1.2")));
    // Asking again keeps the same address.
    assert_eq!(sim.dhcp_acquire(&t, a).unwrap().address, la.address);
    let lb = sim.dhcp_acquire(&t, b).unwrap();
    assert_eq!(lb.address.to_string(), "192.168.1.12/24");
    assert_eq!(sim.state(r).unwrap().leases.len(), 2);
    // The exchange is in the event list.
    assert!(sim.trace().iter().any(|e| e.protocol == Protocol::Dhcp && e.summary.contains("DHCP OFFER")));
    // A server that forgot its leases (a reopened lab) neither hands out an address in use nor
    // moves a host that already has one.
    sim.clear_leases(r);
    let c = add(&mut t, DeviceKind::Pc);
    cable(&mut t, c, 0, s, 3);
    let mut t2 = t.clone();
    t2.device_mut(a).unwrap().ports[0].ip = Some(la.address);
    t2.device_mut(b).unwrap().ports[0].ip = Some(lb.address);
    // The new PC's switch port listens and learns for 30 s first.
    sim.topology_changed(&t2);
    sim.run_for(&t2, 31_000_000);
    assert_eq!(sim.dhcp_acquire(&t2, c).unwrap().address.to_string(), "192.168.1.13/24");
    assert_eq!(sim.dhcp_acquire(&t2, a).unwrap().address, la.address);
    // No pool for the subnet: no answer.
    t.device_mut(r).unwrap().dhcp_pools.clear();
    sim.clear_leases(r);
    assert!(sim.dhcp_acquire(&t, a).is_err());
    // A tiny pool runs out.
    t.device_mut(r).unwrap().dhcp_pools.push(DhcpPool { name: "TINY".into(), network: Some(net("192.168.1.0/29")), default_router: None, dns: None });
    t.device_mut(r).unwrap().dhcp_excluded = vec![(ip("192.168.1.2"), ip("192.168.1.5"))];
    assert_eq!(sim.dhcp_acquire(&t, a).unwrap().address.to_string(), "192.168.1.6/29");
    assert!(sim.dhcp_acquire(&t, b).is_err());
}

#[test]
fn spanning_tree_breaks_a_triangle_of_switches() {
    let mut t = Topology::new();
    let s1 = add(&mut t, DeviceKind::Switch);
    let s2 = add(&mut t, DeviceKind::Switch);
    let s3 = add(&mut t, DeviceKind::Switch);
    // Gigabit links: S1 G0/1 - S2 G0/1, S2 G0/2 - S3 G0/1, S3 G0/2 - S1 G0/2.
    cable(&mut t, s1, 24, s2, 24);
    cable(&mut t, s2, 25, s3, 24);
    cable(&mut t, s3, 25, s1, 25);
    let a = host(&mut t, "10.0.0.1/24", None);
    let b = host(&mut t, "10.0.0.2/24", None);
    cable(&mut t, a, 0, s2, 0);
    cable(&mut t, b, 0, s3, 0);
    let mut sim = Sim::new();
    let v1 = VlanId::DEFAULT;
    let tree = sim.spanning_tree(&t, v1).clone();
    // S1 has the lowest MAC, so it is the root; both its ports are designated.
    assert_eq!(tree.bridges[&s2].root, tree.bridges[&s1].id);
    assert_eq!(tree.ports[&(s1, 24)].role, stp::Role::Designated);
    assert_eq!(tree.ports[&(s1, 25)].role, stp::Role::Designated);
    assert_eq!(tree.ports[&(s2, 24)].role, stp::Role::Root);
    assert_eq!(tree.ports[&(s3, 25)].role, stp::Role::Root);
    // On the S2-S3 link, S2 (lower id at equal cost) is designated and S3 blocks.
    assert_eq!(tree.ports[&(s2, 25)].role, stp::Role::Designated);
    assert_eq!(tree.ports[&(s3, 24)].role, stp::Role::Alternate);
    assert_eq!(tree.ports[&(s2, 0)].role, stp::Role::Designated);
    assert_eq!(tree.bridges[&s3].root_cost, 4);
    // Traffic flows without a storm.
    assert_eq!(marks(&sim.ping(&t, a, ip("10.0.0.2"), PingOptions { count: 3, ..Default::default() }).unwrap()), "!!!");
    assert!(!sim.storm_detected());
    assert!(sim.trace().iter().any(|e| e.dropped.as_deref().is_some_and(|d| d.starts_with("port blocked by spanning tree"))));
    // Making S3 the root moves the blocked port.
    t.device_mut(s3).unwrap().stp_priority.insert(v1, 4096);
    let tree = sim.spanning_tree(&t, v1).clone();
    assert_eq!(tree.bridges[&s1].root, tree.bridges[&s3].id);
    assert_eq!(tree.ports[&(s3, 24)].role, stp::Role::Designated);
    assert!(tree.ports.values().filter(|p| p.role == stp::Role::Alternate).count() == 1);
    // Cutting a link heals the tree: no blocked port is needed any more.
    t.disconnect(End { device: s1, port: 24 });
    sim.topology_changed(&t);
    let tree = sim.spanning_tree(&t, v1).clone();
    assert!(tree.ports.values().all(|p| p.role != stp::Role::Alternate));
    // The port that stops blocking listens and learns for 30 s before it forwards.
    sim.run_for(&t, 31_000_000);
    assert_eq!(marks(&sim.ping(&t, a, ip("10.0.0.2"), PingOptions { count: 2, ..Default::default() }).unwrap()), "!!");
}

#[test]
fn a_switch_vlan_interface_answers_and_reaches_other_subnets() {
    let mut t = Topology::new();
    let r = add(&mut t, DeviceKind::Router);
    let s = add(&mut t, DeviceKind::Switch);
    let a = host(&mut t, "10.0.1.10/24", Some("10.0.1.1"));
    let b = host(&mut t, "10.0.2.10/24", Some("10.0.2.1"));
    router_if(&mut t, r, 0, "10.0.1.1/24");
    router_if(&mut t, r, 1, "10.0.2.1/24");
    cable(&mut t, r, 0, s, 24);
    cable(&mut t, a, 0, s, 0);
    cable(&mut t, b, 0, r, 1);
    {
        let sw = t.device_mut(s).unwrap();
        let svi = sw.svi_mut(VlanId::DEFAULT).unwrap();
        svi.ip = Some(net("10.0.1.2/24"));
        // Shut down by default, as on real switches.
        assert!(svi.shutdown);
    }
    let mut sim = Sim::new();
    assert_eq!(marks(&sim.ping(&t, a, ip("10.0.1.2"), PingOptions { count: 2, ..Default::default() }).unwrap()), "..");
    t.device_mut(s).unwrap().svi_mut(VlanId::DEFAULT).unwrap().shutdown = false;
    sim.topology_changed(&t);
    assert_eq!(marks(&sim.ping(&t, a, ip("10.0.1.2"), PingOptions { count: 3, ..Default::default() }).unwrap()), "!!!");
    // Without a default gateway the switch can't leave its subnet.
    assert!(sim.ping(&t, s, ip("10.0.2.10"), PingOptions::default()).is_err());
    t.device_mut(s).unwrap().gateway = Some(ip("10.0.1.1"));
    sim.ping(&t, s, ip("10.0.2.10"), PingOptions::default()).unwrap();
    assert_eq!(marks(&sim.ping(&t, s, ip("10.0.2.10"), PingOptions { count: 3, ..Default::default() }).unwrap()), "!!!");
    assert_eq!(marks(&sim.ping(&t, b, ip("10.0.1.2"), PingOptions { count: 3, ..Default::default() }).unwrap()), "!!!");
    assert!(routing_table(&t, t.device(s).unwrap()).iter().any(|r| r.code == 'C' && r.iface.as_deref() == Some("Vlan1")));
}

#[test]
fn a_relay_forwards_dhcp_to_a_server_on_another_subnet() {
    use netlabcraft_model::DhcpPool;
    // PC1 - S1 - R1 (relay) ==10.0.0.0/30== R2 (DHCP server)
    let mut t = Topology::new();
    let r1 = add(&mut t, DeviceKind::Router);
    let r2 = add(&mut t, DeviceKind::Router);
    let s = add(&mut t, DeviceKind::Switch);
    let a = add(&mut t, DeviceKind::Pc);
    router_if(&mut t, r1, 0, "192.168.20.1/24");
    router_if(&mut t, r1, 1, "10.0.0.1/30");
    router_if(&mut t, r2, 1, "10.0.0.2/30");
    t.device_mut(r1).unwrap().ports[0].helpers.push(ip("10.0.0.2"));
    {
        let r = t.device_mut(r2).unwrap();
        r.dhcp_pools.push(DhcpPool {
            name: "REMOTE".into(),
            network: Some(net("192.168.20.0/24")),
            default_router: Some(ip("192.168.20.1")),
            dns: None,
        });
        r.routes.push(StaticRoute { net: net("192.168.20.0/24"), via: NextHop::Address(ip("10.0.0.1")) });
    }
    cable(&mut t, r1, 0, s, 0);
    cable(&mut t, a, 0, s, 1);
    cable(&mut t, r1, 1, r2, 1);
    let mut sim = Sim::new();
    // The first exchange may lose a packet while the routers resolve each other with ARP.
    let lease = sim.dhcp_acquire(&t, a).or_else(|_| sim.dhcp_acquire(&t, a)).unwrap();
    assert_eq!(lease.address.to_string(), "192.168.20.2/24");
    assert_eq!(lease.router, Some(ip("192.168.20.1")));
    assert_eq!(sim.state(r2).unwrap().leases.len(), 1);
}

#[test]
fn ospf_routes_around_a_triangle() {
    use netlabcraft_model::{Ospf, OspfNetwork};
    let mut t = Topology::new();
    let r1 = add(&mut t, DeviceKind::Router);
    let r2 = add(&mut t, DeviceKind::Router);
    let r3 = add(&mut t, DeviceKind::Router);
    for r in [r1, r2, r3] {
        let d = t.device_mut(r).unwrap();
        d.model = "Router-4".into();
        let more = netlabcraft_model::Device::of_model(r, netlabcraft_model::models::find("Router-4").unwrap(), d.name.clone(), 0.0, 0.0).ports;
        d.ports = more;
    }
    let a = host(&mut t, "192.168.1.10/24", Some("192.168.1.1"));
    let c = host(&mut t, "192.168.3.10/24", Some("192.168.3.1"));
    router_if(&mut t, r1, 0, "192.168.1.1/24");
    router_if(&mut t, r3, 0, "192.168.3.1/24");
    router_if(&mut t, r1, 1, "10.0.12.1/30");
    router_if(&mut t, r2, 1, "10.0.12.2/30");
    router_if(&mut t, r2, 2, "10.0.23.1/30");
    router_if(&mut t, r3, 2, "10.0.23.2/30");
    router_if(&mut t, r1, 3, "10.0.13.1/30");
    router_if(&mut t, r3, 3, "10.0.13.2/30");
    cable(&mut t, a, 0, r1, 0);
    cable(&mut t, c, 0, r3, 0);
    cable(&mut t, r1, 1, r2, 1);
    cable(&mut t, r2, 2, r3, 2);
    cable(&mut t, r1, 3, r3, 3);
    let all = OspfNetwork { address: ip("0.0.0.0"), wildcard: ip("255.255.255.255"), area: 0 };
    for r in [r1, r2, r3] {
        t.device_mut(r).unwrap().ospf = Some(Ospf { process: 1, networks: vec![all], ..Default::default() });
    }
    // R1's LAN port is passive: no neighbour there, but its network is still advertised.
    t.device_mut(r1).unwrap().ospf.as_mut().unwrap().passive.push("GigabitEthernet0/0".into());
    let mut sim = Sim::new();
    let st = sim.ospf_state(&t);
    assert_eq!(st.neighbors[&r1].len(), 2);
    assert_eq!(st.router_ids[&r1], ip("192.168.1.1"));
    // R1 reaches 192.168.3.0/24 directly over the R1-R3 link (cost 2), not via R2 (cost 3).
    let table = routing_table(&t, t.device(r1).unwrap());
    let o = table.iter().find(|r| r.code == 'O' && r.net.to_string() == "192.168.3.0/24").unwrap();
    assert_eq!(o.via, Some(ip("10.0.13.2")));
    for _ in 0..2 {
        sim.ping(&t, a, ip("192.168.3.10"), PingOptions::default()).unwrap();
    }
    assert_eq!(marks(&sim.ping(&t, a, ip("192.168.3.10"), PingOptions { count: 3, ..Default::default() }).unwrap()), "!!!");
    // Cut R1-R3: traffic goes around through R2.
    t.device_mut(r1).unwrap().ports[3].shutdown = true;
    sim.topology_changed(&t);
    let table = routing_table(&t, t.device(r1).unwrap());
    assert_eq!(table.iter().find(|r| r.net.to_string() == "192.168.3.0/24").unwrap().via, Some(ip("10.0.12.2")));
    for _ in 0..3 {
        sim.ping(&t, a, ip("192.168.3.10"), PingOptions::default()).unwrap();
    }
    assert_eq!(marks(&sim.ping(&t, a, ip("192.168.3.10"), PingOptions { count: 3, ..Default::default() }).unwrap()), "!!!");
    let hops = sim.traceroute(&t, a, ip("192.168.3.10"), 30, 2_000_000).unwrap();
    assert_eq!(hops.len(), 4);
    // A static route beats OSPF for the same network.
    t.device_mut(r1).unwrap().routes.push(StaticRoute { net: net("192.168.3.0/24"), via: NextHop::Address(ip("10.0.12.2")) });
    let table = routing_table(&t, t.device(r1).unwrap());
    assert!(table.iter().any(|r| r.code == 'S' && r.net.to_string() == "192.168.3.0/24"));
    assert!(!table.iter().any(|r| r.code == 'O' && r.net.to_string() == "192.168.3.0/24"));
    // R3 originates its default route.
    {
        let d = t.device_mut(r3).unwrap();
        d.routes.push(StaticRoute { net: net("0.0.0.0/0"), via: NextHop::Interface("GigabitEthernet0/0".into()) });
        d.ospf.as_mut().unwrap().default_originate = true;
    }
    assert!(routing_table(&t, t.device(r2).unwrap()).iter().any(|r| r.code == 'E' && r.net.prefix == 0 && r.via == Some(ip("10.0.23.2"))));
}

#[test]
fn access_lists_filter_forwarded_traffic() {
    use netlabcraft_model::acl::{Acl, AclAddr, AclEntry, AclProto};
    let (mut t, r1, _r2, a, _b) = chain();
    let mut sim = Sim::new();
    for _ in 0..2 {
        sim.ping(&t, a, ip("10.2.0.10"), PingOptions::default()).unwrap();
    }
    // Deny PC1's echoes to PC2 inbound on R1's LAN port.
    {
        let r = t.device_mut(r1).unwrap();
        r.acls.push(Acl {
            name: "110".into(),
            extended: true,
            remarks: Vec::new(),
            entries: vec![
                AclEntry {
                    permit: false,
                    proto: AclProto::Icmp,
                    src: AclAddr::host(ip("10.1.0.10")),
                    src_port: None,
                    dst: AclAddr::host(ip("10.2.0.10")),
                    dst_port: None,
                    seq: 0,
                    established: false,
                    icmp_type: None,
                },
                AclEntry {
                    permit: true,
                    proto: AclProto::Ip,
                    src: AclAddr::ANY,
                    src_port: None,
                    dst: AclAddr::ANY,
                    dst_port: None,
                    seq: 0,
                    established: false,
                    icmp_type: None,
                },
            ],
        });
        r.ports[0].access.inbound = Some("110".into());
    }
    assert_eq!(marks(&sim.ping(&t, a, ip("10.2.0.10"), PingOptions { count: 3, ..Default::default() }).unwrap()), "UUU");
    assert_eq!(sim.state(r1).unwrap().acl_hits.get(&("110".to_string(), 0)), Some(&3));
    // Other destinations still pass (the router itself, through the permit entry).
    assert_eq!(marks(&sim.ping(&t, a, ip("172.16.0.1"), PingOptions { count: 2, ..Default::default() }).unwrap()), "!!");
    // A standard list outbound towards R2: only 10.9.0.0/16 may leave, so PC1 is cut.
    {
        let r = t.device_mut(r1).unwrap();
        r.ports[0].access.inbound = None;
        r.acls.push(Acl {
            name: "5".into(),
            extended: false,
            remarks: Vec::new(),
            entries: vec![AclEntry {
                permit: true,
                proto: AclProto::Ip,
                src: AclAddr { addr: ip("10.9.0.0"), wildcard: ip("0.0.255.255") },
                src_port: None,
                dst: AclAddr::ANY,
                dst_port: None,
                seq: 0,
                established: false,
                icmp_type: None,
            }],
        });
        r.ports[1].access.outbound = Some("5".into());
    }
    assert_eq!(marks(&sim.ping(&t, a, ip("10.2.0.10"), PingOptions { count: 2, ..Default::default() }).unwrap()), "UU");
    assert!(sim.state(r1).unwrap().acl_hits.contains_key(&("5".to_string(), usize::MAX)));
    // An applied list that doesn't exist lets everything through.
    t.device_mut(r1).unwrap().ports[1].access.outbound = Some("77".into());
    assert_eq!(marks(&sim.ping(&t, a, ip("10.2.0.10"), PingOptions { count: 2, ..Default::default() }).unwrap()), "!!");
}

#[test]
fn nat_translates_both_ways() {
    use netlabcraft_model::acl::{Acl, AclAddr, AclEntry, AclProto};
    use netlabcraft_model::{NatRule, NatSide, NatTarget};
    // PC1, PC2 (inside 192.168.1.0/24) - R1 (NAT) - ISP router - SRV 8.8.8.8 network
    let mut t = Topology::new();
    let r1 = add(&mut t, DeviceKind::Router);
    let isp = add(&mut t, DeviceKind::Router);
    let s = add(&mut t, DeviceKind::Switch);
    let a = host(&mut t, "192.168.1.10/24", Some("192.168.1.1"));
    let b = host(&mut t, "192.168.1.11/24", Some("192.168.1.1"));
    let srv = host(&mut t, "8.8.8.8/24", Some("8.8.8.1"));
    router_if(&mut t, r1, 0, "192.168.1.1/24");
    router_if(&mut t, r1, 1, "200.0.0.1/29");
    router_if(&mut t, isp, 1, "200.0.0.2/29");
    router_if(&mut t, isp, 0, "8.8.8.1/24");
    cable(&mut t, r1, 0, s, 0);
    cable(&mut t, a, 0, s, 1);
    cable(&mut t, b, 0, s, 2);
    cable(&mut t, r1, 1, isp, 1);
    cable(&mut t, srv, 0, isp, 0);
    {
        let r = t.device_mut(r1).unwrap();
        r.ports[0].nat = Some(NatSide::Inside);
        r.ports[1].nat = Some(NatSide::Outside);
        r.routes.push(StaticRoute { net: net("0.0.0.0/0"), via: NextHop::Address(ip("200.0.0.2")) });
        r.acls.push(Acl {
            name: "1".into(),
            extended: false,
            remarks: Vec::new(),
            entries: vec![AclEntry {
                permit: true,
                proto: AclProto::Ip,
                src: AclAddr { addr: ip("192.168.1.0"), wildcard: ip("0.0.0.255") },
                src_port: None,
                dst: AclAddr::ANY,
                dst_port: None,
                seq: 0,
                established: false,
                icmp_type: None,
            }],
        });
        r.nat.rules.push(NatRule { acl: "1".into(), target: NatTarget::Interface("GigabitEthernet0/1".into()), overload: true });
    }
    // The ISP knows nothing about 192.168.1.0/24: without NAT the replies couldn't come back.
    let mut sim = Sim::new();
    for h in [a, b] {
        sim.ping(&t, h, ip("8.8.8.8"), PingOptions::default()).unwrap();
        assert_eq!(marks(&sim.ping(&t, h, ip("8.8.8.8"), PingOptions { count: 3, ..Default::default() }).unwrap()), "!!!");
    }
    // The server only ever saw the router's outside address.
    assert!(sim.trace().iter().filter(|e| e.to.device == srv).all(|e| !e.summary.contains("192.168.1.")));
    let nat = &sim.state(r1).unwrap().nat;
    assert!(nat.iter().any(|e| e.inside_local == ip("192.168.1.10")) && nat.iter().any(|e| e.inside_local == ip("192.168.1.11")));
    assert!(nat.iter().all(|e| e.inside_global == ip("200.0.0.1")));
    // A static translation publishes PC1 as 200.0.0.5: the outside can reach it.
    t.device_mut(r1).unwrap().nat.statics.push((ip("192.168.1.10"), ip("200.0.0.5")));
    t.device_mut(isp).unwrap().routes.clear();
    sim.clear_nat(r1);
    sim.ping(&t, srv, ip("200.0.0.5"), PingOptions::default()).unwrap();
    assert_eq!(marks(&sim.ping(&t, srv, ip("200.0.0.5"), PingOptions { count: 3, ..Default::default() }).unwrap()), "!!!");
}

#[test]
fn a_server_answers_dns_http_and_dhcp() {
    use netlabcraft_model::services::{DnsRecord, ServerPool};
    let mut t = Topology::new();
    let s = add(&mut t, DeviceKind::Switch);
    let srv = add(&mut t, DeviceKind::Server);
    let a = add(&mut t, DeviceKind::Pc);
    let r = add(&mut t, DeviceKind::Router);
    let far = host(&mut t, "10.9.0.10/24", Some("10.9.0.1"));
    router_if(&mut t, r, 0, "192.168.5.1/24");
    router_if(&mut t, r, 1, "10.9.0.1/24");
    cable(&mut t, srv, 0, s, 0);
    cable(&mut t, a, 0, s, 1);
    cable(&mut t, r, 0, s, 2);
    cable(&mut t, far, 0, r, 1);
    {
        let d = t.device_mut(srv).unwrap();
        d.ports[0].ip = Some(net("192.168.5.2/24"));
        d.gateway = Some(ip("192.168.5.1"));
        d.services.dns.on = true;
        d.services.dns.records.push(DnsRecord { name: "www.lab.test".into(), address: ip("192.168.5.2") });
        d.services.dhcp.on = true;
        d.services.dhcp.pools = vec![ServerPool {
            name: "lan".into(),
            gateway: ip("192.168.5.1"),
            dns: ip("192.168.5.2"),
            start: ip("192.168.5.100"),
            mask: ip("255.255.255.0"),
            max_users: 20,
        }];
    }
    let mut sim = Sim::new();
    // DHCP from the server.
    let lease = sim.dhcp_acquire(&t, a).unwrap();
    assert_eq!(lease.address.to_string(), "192.168.5.100/24");
    assert_eq!(lease.dns, Some(ip("192.168.5.2")));
    {
        let d = t.device_mut(a).unwrap();
        d.ports[0].ip = Some(lease.address);
        d.gateway = lease.router;
        d.dns = lease.dns;
    }
    // DNS, then HTTP over TCP.
    assert_eq!(sim.dns_resolve(&t, a, "WWW.lab.test").unwrap(), ip("192.168.5.2"));
    assert!(sim.dns_resolve(&t, a, "nope.lab.test").is_err());
    let (status, body) = sim.http_get(&t, a, ip("192.168.5.2"), "www.lab.test", "/").unwrap();
    assert_eq!(status, "200 OK");
    assert!(body.contains("NetLabCraft web server"), "{body}");
    let (status, _) = sim.http_get(&t, a, ip("192.168.5.2"), "www.lab.test", "/missing.html").unwrap();
    assert_eq!(status, "404 Not Found");
    assert!(sim.trace().iter().any(|e| e.protocol == Protocol::Tcp && e.summary.contains("SYN")));
    assert!(sim.trace().iter().any(|e| e.protocol == Protocol::Http && e.summary.contains("GET /")));
    assert!(sim.trace().iter().any(|e| e.protocol == Protocol::Dns));
    // Across a router, too.
    t.device_mut(far).unwrap().dns = Some(ip("192.168.5.2"));
    let ip_www = sim.dns_resolve(&t, far, "www.lab.test").unwrap();
    assert_eq!(sim.http_get(&t, far, ip_www, "www.lab.test", "/about.html").unwrap().0, "200 OK");
    // Web service off: refused.
    t.device_mut(srv).unwrap().services.http.on = false;
    assert!(sim.http_get(&t, a, ip("192.168.5.2"), "x", "/").unwrap_err().to_string().contains("refused"));
    // An access list on the router can stop web traffic by port.
    use netlabcraft_model::acl::{Acl, AclAddr, AclEntry, AclProto, PortMatch};
    t.device_mut(srv).unwrap().services.http.on = true;
    {
        let rt = t.device_mut(r).unwrap();
        rt.acls.push(Acl {
            name: "120".into(),
            extended: true,
            remarks: Vec::new(),
            entries: vec![
                AclEntry {
                    permit: false,
                    proto: AclProto::Tcp,
                    src: AclAddr::ANY,
                    src_port: None,
                    dst: AclAddr::ANY,
                    dst_port: Some(PortMatch::Eq(80)),
                    seq: 0,
                    established: false,
                    icmp_type: None,
                },
                AclEntry {
                    permit: true,
                    proto: AclProto::Ip,
                    src: AclAddr::ANY,
                    src_port: None,
                    dst: AclAddr::ANY,
                    dst_port: None,
                    seq: 0,
                    established: false,
                    icmp_type: None,
                },
            ],
        });
        rt.ports[1].access.inbound = Some("120".into());
    }
    assert!(sim.http_get(&t, far, ip_www, "www.lab.test", "/").is_err());
    assert!(sim.dns_resolve(&t, far, "www.lab.test").is_ok());
}

#[test]
fn straight_and_cross_over_cables_need_auto_mdix_or_the_right_kind() {
    use netlabcraft_model::cable::Cable;
    let wire = |t: &mut Topology, a: DeviceId, b: DeviceId, pb: usize, c: Cable| {
        t.connect_with(End { device: a, port: 0 }, End { device: b, port: pb }, c).unwrap();
    };
    // Two PCs: no auto-MDIX on hosts, so only a cross-over works.
    for (c, works) in [(Cable::Straight, false), (Cable::Cross, true), (Cable::Auto, true)] {
        let mut t = Topology::new();
        let a = host(&mut t, "10.0.0.1/24", None);
        let b = host(&mut t, "10.0.0.2/24", None);
        wire(&mut t, a, b, 0, c);
        let end = End { device: a, port: 0 };
        assert_eq!(port_up(&t, end), works, "{c:?}");
        assert_eq!(cable_problem(&t, end).is_some_and(|w| w.contains("cross-over")), !works);
        let r = Sim::new().ping(&t, a, ip("10.0.0.2"), PingOptions::default()).unwrap();
        assert_eq!(marks(&r) == "!!!!!", works, "{c:?}");
    }
    // A PC on a switch with a cross-over: the switch's auto-MDIX fixes it, until it is off.
    let mut t = Topology::new();
    let s = add(&mut t, DeviceKind::Switch);
    let a = host(&mut t, "10.0.0.1/24", None);
    wire(&mut t, a, s, 0, Cable::Cross);
    let end = End { device: a, port: 0 };
    assert!(port_up(&t, end));
    t.device_mut(s).unwrap().ports[0].mdix_off = true;
    assert!(!port_up(&t, end));
    assert!(cable_problem(&t, end).unwrap().contains("straight-through"));
    // A straight-through between two routers is down only with auto-MDIX off at both ends.
    let mut t = Topology::new();
    let r1 = add(&mut t, DeviceKind::Router);
    let r2 = add(&mut t, DeviceKind::Router);
    router_if(&mut t, r1, 0, "10.0.0.1/30");
    router_if(&mut t, r2, 0, "10.0.0.2/30");
    wire(&mut t, r1, r2, 0, Cable::Straight);
    let end = End { device: r1, port: 0 };
    assert!(port_up(&t, end));
    t.device_mut(r1).unwrap().ports[0].mdix_off = true;
    assert!(port_up(&t, end));
    t.device_mut(r2).unwrap().ports[0].mdix_off = true;
    assert!(!port_up(&t, end));
}

#[test]
fn fiber_reaches_two_kilometres_and_copper_a_hundred_metres() {
    use netlabcraft_model::cable::Cable;
    let mut t = Topology::new();
    let a = t.add_model(netlabcraft_model::models::find("Switch-24-SFP").unwrap(), None, 0.0, 0.0).unwrap();
    let b = add(&mut t, DeviceKind::Switch);
    let (ea, eb) = (End { device: a, port: 24 }, End { device: b, port: 24 });
    t.connect_with(ea, eb, Cable::Fiber).unwrap();
    t.set_cable_length(ea, Some(1500)).unwrap();
    assert!(port_up(&t, ea), "{:?}", cable_problem(&t, ea));
    t.set_cable_length(ea, Some(2500)).unwrap();
    assert!(cable_problem(&t, ea).unwrap().contains("fiber, max 2000 m"));
    let (ca, cb) = (End { device: a, port: 0 }, End { device: b, port: 0 });
    t.connect_with(ca, cb, Cable::Cross).unwrap();
    t.set_cable_length(ca, Some(150)).unwrap();
    assert!(cable_problem(&t, ca).unwrap().contains("copper, max 100 m"));
}
