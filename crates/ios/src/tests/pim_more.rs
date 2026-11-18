//! `show ip rpf` and `ip multicast ttl-threshold`.

use super::pim::{dump, net};

#[test]
fn show_ip_rpf_names_the_interface_and_neighbour_towards_an_address() {
    let mut n = net("sparse-mode");
    let towards_source = n.lab.run(n.r2, "show ip rpf 10.1.1.10");
    assert!(towards_source.contains("Interface: GigabitEthernet0/1") && towards_source.contains("PIM neighbour: 10.0.12.1"), "{towards_source}");
    let on_link = n.lab.run(n.r1, "show ip rpf 10.1.1.10");
    assert!(on_link.contains("Interface: GigabitEthernet0/0") && on_link.contains("on the link"), "{on_link}");
    assert!(n.lab.run(n.r1, "show ip rpf 1.1.1.1").contains("address of this router"));
    assert!(n.lab.run(n.r2, "show ip rpf 172.16.0.1").contains("No route"));
    n.lab.run(n.r2, "conf t\ninterface g0/1\nno ip pim sparse-mode\nend");
    assert!(n.lab.run(n.r2, "show ip rpf 10.1.1.10").contains("PIM does not run"));
}

#[test]
fn a_ttl_threshold_holds_back_packets_with_a_low_ttl() {
    let mut n = net("sparse-mode");
    n.lab.run(n.rcv, "igmp join 239.1.1.1");
    // The ping leaves the PC with TTL 128, so a threshold of 200 holds it at R1, and 100 lets it by.
    n.lab.run(n.r1, "conf t\ninterface g0/1\nip multicast ttl-threshold 200\nend");
    let held = n.lab.run(n.src, "ping -n 1 239.1.1.1");
    assert!(held.contains("no answer"), "{held}\n{}", dump(&n.lab));
    assert!(n.lab.run(n.r1, "show running-config").contains(" ip multicast ttl-threshold 200"));
    n.lab.run(n.r1, "conf t\ninterface g0/1\nip multicast ttl-threshold 100\nend");
    let passed = n.lab.run(n.src, "ping -n 1 239.1.1.1");
    assert!(passed.contains("reply from 10.2.2.10"), "{passed}\n{}", dump(&n.lab));
    n.lab.run(n.r1, "conf t\ninterface g0/1\nno ip multicast ttl-threshold\nend");
    assert!(!n.lab.run(n.r1, "show running-config").contains("ttl-threshold"));
    // The setting belongs to an interface that runs PIM.
    n.lab.run(n.r1, "conf t\ninterface g0/1\nno ip pim sparse-mode");
    assert!(n.lab.try_run(n.r1, "ip multicast ttl-threshold 5").starts_with("% "));
}
