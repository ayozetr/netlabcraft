//! Router advertisements the console switches off or shortens: `ipv6 nd ra suppress` and
//! `ipv6 nd ra lifetime`.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

/// R1 with 2001:db8:1::1/64 on g0/0 and a PC that has not asked for an address yet.
fn lan() -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    let pc = lab.add(DeviceKind::Pc);
    lab.cable(pc, "fa0", r, "g0/0");
    lab.run(r, "en\nconf t\nipv6 unicast-routing\nint g0/0\nipv6 address 2001:db8:1::1/64\nno shut\nend");
    (lab, r, pc)
}

#[test]
fn a_router_that_suppresses_advertisements_leaves_hosts_without_an_address() {
    let (mut lab, r, pc) = lan();
    lab.run(r, "conf t\nint g0/0\nipv6 nd ra suppress all\nend");
    assert!(lab.run(r, "show running-config").contains(" ipv6 nd ra suppress all\n"));
    let none = lab.try_run(pc, "ipv6config auto");
    assert!(!none.contains("Autoconfigured"), "no advertisement, no address: {none}");
    lab.run(r, "conf t\nint g0/0\nno ipv6 nd ra suppress\nend");
    assert!(!lab.run(r, "show running-config").contains("ra suppress"));
    let some = lab.run(pc, "ipv6config auto");
    assert!(some.contains("Autoconfigured 2001:db8:1::"), "{some}");
}

#[test]
fn a_router_lifetime_of_zero_says_the_router_is_no_default_router() {
    let (mut lab, r, pc) = lan();
    lab.run(r, "conf t\nint g0/0\nipv6 nd ra lifetime 0\nend");
    assert!(lab.run(r, "show running-config").contains(" ipv6 nd ra lifetime 0\n"));
    assert!(!lab.try_run(pc, "ipv6config auto").contains("Autoconfigured"), "a lifetime of 0 is not taken as a router");
    lab.run(r, "conf t\nint g0/0\nipv6 nd ra lifetime 600\nend");
    assert!(lab.run(pc, "ipv6config auto").contains("Autoconfigured 2001:db8:1::"));
    lab.run(r, "conf t\nint g0/0");
    assert!(lab.try_run(r, "ipv6 nd ra lifetime 9001").starts_with("% The router lifetime"));
    lab.run(r, "no ipv6 nd ra lifetime\nend");
    assert!(!lab.run(r, "show running-config").contains("ra lifetime"));
}
