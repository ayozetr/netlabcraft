//! Settings that used to be only kept and shown and now change what the simulator does.

use netlabcraft_model::DeviceKind;

use super::Lab;

#[test]
fn no_ip_unreachables_keeps_a_router_from_answering_with_unreachables() {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.run(r1, "en\nconf t\nint g0/0\nip address 10.0.0.1 255.255.255.0\nno shut\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip address 10.0.0.2 255.255.255.0\nno shut\nexit\nip route 0.0.0.0 0.0.0.0 10.0.0.1\nend");
    let with = lab.run(r2, "ping 10.9.9.9");
    assert!(with.contains("UUU"), "R1 has no route and says so: {with}");
    lab.run(r1, "conf t\nint g0/0\nno ip unreachables\nend");
    let without = lab.run(r2, "ping 10.9.9.9");
    assert!(!without.contains("UU"), "R1 stays silent: {without}");
    lab.run(r1, "conf t\nint g0/0\nip unreachables\nend");
    assert!(lab.run(r2, "ping 10.9.9.9").contains("UUU"), "turned back on");
}
