//! Message-digest key rollover and key chains on OSPF interfaces.

use netlabcraft_wire::control::{self, OspfAuth};

use super::Lab;
use super::console_depth::{ospf_auth_on_wire, ospf_pair, settle};

fn full(lab: &mut Lab, r: netlabcraft_core::DeviceId) -> bool {
    lab.run(r, "show ip ospf neighbor").contains("FULL")
}

#[test]
fn a_router_holding_two_keys_is_heard_by_a_neighbour_that_has_only_the_old_one() {
    let (mut lab, r1, r2) = ospf_pair(
        "ip ospf authentication message-digest\nip ospf message-digest-key 1 md5 sk-one",
        "ip ospf authentication message-digest\nip ospf message-digest-key 1 md5 sk-one",
    );
    settle(&mut lab);
    assert!(full(&mut lab, r1));
    // Step one of a rollover: the new key goes on one router. It keeps signing with both.
    lab.run(r1, "conf t\ninterface g0/0\nip ospf message-digest-key 2 md5 sk-two\nend");
    settle(&mut lab);
    assert!(full(&mut lab, r1) && full(&mut lab, r2), "the old key still reaches the neighbour");
    let ids: std::collections::BTreeSet<u8> =
        ospf_auth_on_wire(&lab).into_iter().filter_map(|(a, _)| if let OspfAuth::Md5 { key_id, .. } = a { Some(key_id) } else { None }).collect();
    assert!(ids.contains(&1) && ids.contains(&2), "one copy per key: {ids:?}");
    assert!(lab.run(r1, "show ip ospf interface g0/0").contains("signing with key 2 (and 1 older key)"));
    // Step two: the new key on the other router, then the old one off the first.
    lab.run(r2, "conf t\ninterface g0/0\nip ospf message-digest-key 2 md5 sk-two\nend");
    lab.run(r1, "conf t\ninterface g0/0\nno ip ospf message-digest-key 1\nend");
    settle(&mut lab);
    assert!(full(&mut lab, r1) && full(&mut lab, r2));
    for (auth, packet) in ospf_auth_on_wire(&lab) {
        if let OspfAuth::Md5 { key_id: 2, .. } = auth {
            assert!(control::ospf_md5_verify(&packet, "sk-two"));
        }
    }
    // A key only one side has leaves them apart.
    lab.run(r2, "conf t\ninterface g0/0\nno ip ospf message-digest-key 2\nend");
    settle(&mut lab);
    assert!(!full(&mut lab, r1));
}

#[test]
fn a_key_chain_signs_and_its_lifetimes_follow_the_devices_clock() {
    let (mut lab, r1, r2) =
        ospf_pair("ip ospf authentication key-chain RING", "ip ospf authentication message-digest\nip ospf message-digest-key 1 md5 sk-test");
    assert!(lab.run(r1, "show running-config").contains(" ip ospf authentication key-chain RING"));
    // The chain does not exist yet: no key, no adjacency.
    settle(&mut lab);
    assert!(!full(&mut lab, r1));
    // The key may only sign from 2030; the simulated clock starts in 2026.
    lab.run(
        r1,
        "conf t\nkey chain RING\nkey 1\nkey-string sk-test\nsend-lifetime 00:00:00 Jan 1 2030 infinite\naccept-lifetime 00:00:00 Jan 1 2020 infinite\nend",
    );
    settle(&mut lab);
    assert!(!full(&mut lab, r1), "no key signs yet");
    let shown = lab.run(r1, "show key chain RING");
    assert!(shown.contains("key 1") && shown.contains("not yet valid") && shown.contains("no key signs now"), "{shown}");
    // Setting the clock past the start makes it valid.
    lab.run(r1, "clock set 12:00:00 2 Jan 2030");
    settle(&mut lab);
    assert!(full(&mut lab, r1) && full(&mut lab, r2));
    let shown = lab.run(r1, "show key chain RING");
    assert!(shown.contains("signing now with key 1"), "{shown}");
    assert!(lab.run(r1, "show ip ospf interface g0/0").contains("message digest, key chain RING, signing with key 1"));
    let cfg = lab.run(r1, "show running-config");
    for line in [
        "key chain RING",
        " key 1",
        "  key-string sk-test",
        "  send-lifetime 00:00:00 Jan 1 2030 infinite",
        "  accept-lifetime 00:00:00 Jan 1 2020 infinite",
    ] {
        assert!(cfg.lines().any(|l| l == line), "missing `{line}`:\n{cfg}");
    }
    // Back to the manual keys.
    lab.run(r1, "conf t\ninterface g0/0\nno ip ospf authentication key-chain\nend");
    assert!(!lab.run(r1, "show running-config").contains("key-chain RING"));
}

#[test]
fn a_key_that_expired_is_no_longer_accepted() {
    let (mut lab, r1, r2) =
        ospf_pair("ip ospf authentication key-chain RING", "ip ospf authentication message-digest\nip ospf message-digest-key 1 md5 sk-test");
    lab.run(
        r1,
        "conf t\nkey chain RING\nkey 1\nkey-string sk-test\naccept-lifetime 00:00:00 Jan 1 2020 00:00:00 Jun 1 2026\nsend-lifetime 00:00:00 Jan 1 2020 infinite\nend",
    );
    lab.run(r1, "clock set 12:00:00 1 Mar 2026");
    settle(&mut lab);
    assert!(full(&mut lab, r1) && full(&mut lab, r2));
    lab.run(r1, "clock set 12:00:00 1 Jul 2026");
    settle(&mut lab);
    assert!(!full(&mut lab, r1), "the accept lifetime ended in June");
}

#[test]
fn key_chain_commands_check_their_arguments_and_leave_with_exit() {
    let (mut lab, r1, _) = ospf_pair("", "");
    lab.run(r1, "conf t\nkey chain C");
    assert!(lab.prompt(r1).ends_with("(config-keychain)#"), "{}", lab.prompt(r1));
    lab.run(r1, "key 7");
    assert!(lab.prompt(r1).ends_with("(config-keychain-key)#"), "{}", lab.prompt(r1));
    assert!(lab.try_run(r1, "accept-lifetime 25:00:00 Jan 1 2026 infinite").starts_with("% "));
    assert!(lab.try_run(r1, "send-lifetime 00:00:00 Jan 1 2026").starts_with("% "));
    assert!(lab.try_run(r1, "send-lifetime 00:00:00 Jan 2 2026 00:00:00 Jan 1 2026").contains("end after it starts"));
    lab.run(r1, "send-lifetime 00:00:00 Jan 1 2026 duration 3600");
    lab.run(r1, "exit");
    assert!(lab.prompt(r1).ends_with("(config-keychain)#"), "{}", lab.prompt(r1));
    assert!(lab.try_run(r1, "no key 9").starts_with("% "));
    lab.run(r1, "exit");
    assert!(lab.prompt(r1).ends_with("(config)#"), "{}", lab.prompt(r1));
    assert!(lab.try_run(r1, "no key chain NOPE").starts_with("% "));
    lab.run(r1, "no key chain C\nend");
    assert!(lab.run(r1, "show key chain").contains("No key chains"));
}
