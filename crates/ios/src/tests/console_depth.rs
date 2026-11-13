//! Typed console tests for the second round of console depth (M9.7).

use netlabcraft_model::DeviceKind;
use netlabcraft_wire::control::{self, OspfAuth};
use netlabcraft_wire::{Frame, Ipv4};

use super::Lab;

/// Two routers on one link, OSPF area 0, the given extra lines typed on both (interface lines
/// first, in `g0/0`, then router lines).
fn ospf_pair(extra_a: &str, extra_b: &str) -> (Lab, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    for (r, ip, id, extra) in [(r1, "10.0.0.1", "1.1.1.1", extra_a), (r2, "10.0.0.2", "2.2.2.2", extra_b)] {
        lab.run(
            r,
            &format!(
                "en\nconf t\ninterface g0/0\nip address {ip} 255.255.255.0\nno shutdown\n{extra}\nexit\nrouter ospf 1\nrouter-id {id}\nnetwork 10.0.0.0 0.0.0.255 area 0\nend"
            ),
        );
    }
    (lab, r1, r2)
}

/// The authentication fields of the OSPF packets on the wire.
fn ospf_auth_on_wire(lab: &Lab) -> Vec<(OspfAuth, Vec<u8>)> {
    lab.sim
        .trace()
        .iter()
        .filter_map(|e| {
            let ip = Ipv4::decode(&Frame::decode(&e.frame).ok()?.payload).ok()?;
            (ip.protocol == control::PROTO_OSPF).then(|| (control::ospf_auth_of(&ip.payload), ip.payload))
        })
        .filter_map(|(a, p)| Some((a?, p)))
        .collect()
}

fn settle(lab: &mut Lab) {
    lab.sim.run_for(&lab.t, 60_000_000);
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 30_000_000);
}

#[test]
fn ospf_neighbours_with_the_same_message_digest_key_form_and_sign_every_packet() {
    let (mut lab, r1, _r2) = ospf_pair(
        "ip ospf authentication message-digest\nip ospf message-digest-key 1 md5 sk-test",
        "ip ospf authentication message-digest\nip ospf message-digest-key 1 md5 sk-test",
    );
    settle(&mut lab);
    assert!(lab.run(r1, "show ip ospf neighbor").contains("FULL"));
    let wire = ospf_auth_on_wire(&lab);
    assert!(!wire.is_empty());
    for (auth, packet) in &wire {
        assert!(matches!(auth, OspfAuth::Md5 { key_id: 1, .. }), "{auth:?}");
        assert!(control::ospf_md5_verify(packet, "sk-test"), "digest must verify");
        assert!(!control::ospf_md5_verify(packet, "sk-wrong"));
    }
    assert!(lab.run(r1, "show ip ospf interface g0/0").contains("Authentication: message digest, signing with key 1"));
    let cfg = lab.run(r1, "show running-config");
    assert!(cfg.contains(" ip ospf authentication message-digest") && cfg.contains(" ip ospf message-digest-key 1 md5 sk-test"), "{cfg}");
}

#[test]
fn ospf_neighbours_with_a_different_key_or_type_do_not_form_an_adjacency() {
    // A different key under the same id.
    let (mut lab, r1, r2) = ospf_pair(
        "ip ospf authentication message-digest\nip ospf message-digest-key 1 md5 sk-test",
        "ip ospf authentication message-digest\nip ospf message-digest-key 1 md5 sk-other",
    );
    settle(&mut lab);
    assert!(!lab.run(r1, "show ip ospf neighbor").contains("FULL"));
    assert!(!lab.run(r2, "show ip ospf neighbor").contains("FULL"));
    // Same key, different key id.
    let (mut lab, r1, _) = ospf_pair(
        "ip ospf authentication message-digest\nip ospf message-digest-key 1 md5 sk-test",
        "ip ospf authentication message-digest\nip ospf message-digest-key 2 md5 sk-test",
    );
    settle(&mut lab);
    assert!(!lab.run(r1, "show ip ospf neighbor").contains("FULL"));
    // Digest against nothing, password against digest.
    for b in ["", "ip ospf authentication\nip ospf authentication-key sk-test"] {
        let (mut lab, r1, _) = ospf_pair("ip ospf authentication message-digest\nip ospf message-digest-key 1 md5 sk-test", b);
        settle(&mut lab);
        assert!(!lab.run(r1, "show ip ospf neighbor").contains("FULL"), "{b}");
    }
    // Digest type with no key at all signs nothing.
    let (mut lab, r1, _) =
        ospf_pair("ip ospf authentication message-digest", "ip ospf authentication message-digest\nip ospf message-digest-key 1 md5 sk-test");
    settle(&mut lab);
    assert!(!lab.run(r1, "show ip ospf neighbor").contains("FULL"));
}

#[test]
fn ospf_simple_passwords_must_match_and_cross_the_wire_in_clear() {
    let (mut lab, r1, _) =
        ospf_pair("ip ospf authentication\nip ospf authentication-key sk-test", "ip ospf authentication\nip ospf authentication-key sk-test");
    settle(&mut lab);
    assert!(lab.run(r1, "show ip ospf neighbor").contains("FULL"));
    assert!(ospf_auth_on_wire(&lab).iter().all(|(a, _)| *a == OspfAuth::Simple(*b"sk-test\0")));
    let (mut lab, r1, _) =
        ospf_pair("ip ospf authentication\nip ospf authentication-key sk-test", "ip ospf authentication\nip ospf authentication-key sk-xxxx");
    settle(&mut lab);
    assert!(!lab.run(r1, "show ip ospf neighbor").contains("FULL"));
}

#[test]
fn area_authentication_applies_to_interfaces_and_an_interface_setting_wins() {
    let (mut lab, r1, r2) = ospf_pair("ip ospf message-digest-key 1 md5 sk-test", "ip ospf message-digest-key 1 md5 sk-test");
    lab.run(r1, "conf t\nrouter ospf 1\narea 0 authentication message-digest\nend");
    // Only one end authenticates: no adjacency.
    settle(&mut lab);
    assert!(!lab.run(r1, "show ip ospf neighbor").contains("FULL"));
    lab.run(r2, "conf t\nrouter ospf 1\narea 0 authentication message-digest\nend");
    settle(&mut lab);
    assert!(lab.run(r1, "show ip ospf neighbor").contains("FULL"));
    assert!(lab.run(r1, "show running-config").contains(" area 0 authentication message-digest"));
    // An interface set to null opts out of the area's setting, and then disagrees with the other end.
    lab.run(r1, "conf t\ninterface g0/0\nip ospf authentication null\nend");
    settle(&mut lab);
    assert!(!lab.run(r1, "show ip ospf neighbor").contains("FULL"));
    lab.run(r1, "conf t\ninterface g0/0\nno ip ospf authentication\nno ip ospf message-digest-key 1\nip ospf message-digest-key 3 md5 sk-test\nend");
    settle(&mut lab);
    assert!(!lab.run(r1, "show ip ospf neighbor").contains("FULL"), "key id 3 is not key id 1");
    lab.run(r2, "conf t\ninterface g0/0\nip ospf message-digest-key 3 md5 sk-test\nend");
    settle(&mut lab);
    assert!(lab.run(r1, "show ip ospf neighbor").contains("FULL"));
    lab.run(r1, "conf t\nrouter ospf 1\nno area 0 authentication\nend");
    assert!(!lab.run(r1, "show running-config").contains("area 0 authentication"));
}

#[test]
fn ospf_authentication_commands_reject_bad_arguments() {
    let (mut lab, r1, _) = ospf_pair("", "");
    lab.run(r1, "conf t\ninterface g0/0");
    for bad in [
        "ip ospf authentication bogus",
        "ip ospf message-digest-key 0 md5 k",
        "ip ospf message-digest-key 256 md5 k",
        "ip ospf message-digest-key 1 md5 12345678901234567",
        "ip ospf authentication-key 123456789",
    ] {
        assert!(lab.try_run(r1, bad).starts_with("% "), "{bad}");
    }
    for n in 1..=16 {
        lab.run(r1, &format!("ip ospf message-digest-key {n} md5 k"));
    }
    assert!(lab.try_run(r1, "ip ospf message-digest-key 17 md5 k").starts_with("% Too many"));
}
