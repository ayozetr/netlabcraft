//! Switching and routing leftovers on the sample labs: CoS marking by IP phones and the
//! switch's trust of it.

use serde_json::json;

use crate::Session;

fn cli(s: &mut Session, dev: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

fn lines(s: &mut Session, dev: &str, text: &str) -> String {
    let mut last = String::new();
    for l in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        last = cli(s, dev, l);
        assert!(!last.starts_with("% "), "`{l}` on {dev}: {last}");
    }
    last
}

/// (CoS, DSCP) of the tagged IPv4 frames S1 sent `to` whose summary contains `what`.
fn marks_to(s: &Session, to: &str, what: &str) -> Vec<(u8, u8)> {
    let s1 = s.topology.find("S1").unwrap().id;
    let r1 = s.topology.find(to).unwrap().id;
    s.sim
        .trace()
        .iter()
        .filter(|e| e.from.device == s1 && e.to.device == r1 && e.summary.contains(what))
        .filter(|e| e.frame.get(12..14) == Some(&[0x81, 0x00][..]) && e.frame.get(16..18) == Some(&[0x08, 0x00][..]))
        .map(|e| (netlabcraft_wire::qos::frame_pcp(&e.frame), e.frame.get(19).copied().unwrap_or(0) >> 2))
        .collect()
}

/// [`marks_to`] towards the router.
fn marks(s: &Session, what: &str) -> Vec<(u8, u8)> {
    marks_to(s, "R1", what)
}

/// A call between the two phones, three seconds of voice.
fn talk(s: &mut Session) {
    let n2 = s.execute("phone.status", json!({"device": "Phone2"})).unwrap()["number"].as_str().unwrap().to_string();
    s.execute("phone.dial", json!({"device": "Phone1", "number": n2})).unwrap();
    s.execute("phone.answer", json!({"device": "Phone2"})).unwrap();
    s.execute("sim.advance", json!({"seconds": 3})).unwrap();
}

#[test]
fn phones_mark_voice_cos_5_and_trust_cos_maps_it_to_dscp() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "ip-phones"})).unwrap();
    s.sim.clear_trace();
    talk(&mut s);
    // QoS off: the phone's own marks (CoS 5, EF) cross the switch as they are.
    let m = marks_to(&s, "Phone2", ":16384");
    assert!(!m.is_empty() && m.iter().all(|x| *x == (5, 46)), "{m:?}");
    assert!(s.sim.trace().iter().any(|e| e.summary.contains("[VLAN 100, CoS 5]")), "the event list shows the priority");
    s.execute("phone.hangup", json!({"device": "Phone2"})).unwrap();
    // QoS on, the phone's port untrusted: DSCP 0 and CoS 0 towards the router.
    lines(&mut s, "S1", "enable\nconfigure terminal\nmls qos\nend");
    s.sim.clear_trace();
    talk(&mut s);
    let m = marks_to(&s, "Phone2", ":16384");
    assert!(!m.is_empty() && m.iter().all(|x| *x == (0, 0)), "{m:?}");
    s.execute("phone.hangup", json!({"device": "Phone2"})).unwrap();
    // Trust CoS: CoS 5 maps to DSCP 40 by default…
    lines(&mut s, "S1", "configure terminal\ninterface range fa0/1 - 2\nmls qos trust cos\nend");
    s.sim.clear_trace();
    talk(&mut s);
    let m = marks_to(&s, "Phone2", ":16384");
    assert!(!m.is_empty() && m.iter().all(|x| *x == (5, 40)), "{m:?}");
    s.execute("phone.hangup", json!({"device": "Phone2"})).unwrap();
    // …and to EF with the usual voice map.
    lines(&mut s, "S1", "configure terminal\nmls qos map cos-dscp 0 8 16 24 32 46 48 56\nend");
    let maps = cli(&mut s, "S1", "show mls qos maps cos-dscp");
    assert!(maps.contains("configured") && maps.contains("32 46 48"), "{maps}");
    s.sim.clear_trace();
    talk(&mut s);
    let m = marks_to(&s, "Phone2", ":16384");
    assert!(!m.is_empty() && m.iter().all(|x| *x == (5, 46)), "{m:?}");
    s.execute("phone.hangup", json!({"device": "Phone2"})).unwrap();
    // The PC behind the phone: CoS 0 until the phone is told to give it another.
    cli(&mut s, "PC1", "ping 192.168.10.1");
    s.sim.clear_trace();
    cli(&mut s, "PC1", "ping 192.168.10.1");
    let m = marks(&s, "echo request");
    assert!(!m.is_empty() && m.iter().all(|x| *x == (0, 0)), "{m:?}");
    lines(&mut s, "S1", "configure terminal\ninterface fa0/1\nswitchport priority extend cos 2\nend");
    s.sim.clear_trace();
    cli(&mut s, "PC1", "ping 192.168.10.1");
    let m = marks(&s, "echo request");
    assert!(!m.is_empty() && m.iter().all(|x| *x == (2, 16)), "{m:?}");
    // A port's own CoS for untagged frames, through the map.
    lines(&mut s, "S1", "configure terminal\ninterface fa0/1\nno switchport priority extend\nmls qos cos 3\nend");
    s.sim.clear_trace();
    cli(&mut s, "PC1", "ping 192.168.10.1");
    let m = marks(&s, "echo request");
    assert!(!m.is_empty() && m.iter().all(|x| *x == (3, 24)), "{m:?}");
    // The configuration reads back and survives a reload.
    let run = cli(&mut s, "S1", "show running-config");
    for l in ["\nmls qos\nmls qos map cos-dscp 0 8 16 24 32 46 48 56\n", " mls qos trust cos\n mls qos cos 3\n"] {
        assert!(run.contains(l), "{l}\n{run}");
    }
    lines(&mut s, "S1", "copy running-config startup-config\nreload");
    lines(&mut s, "S1", "enable");
    assert_eq!(cli(&mut s, "S1", "show running-config"), run);
    lines(&mut s, "S1", "configure terminal\nno mls qos map cos-dscp\ninterface fa0/1\nno mls qos cos\nend");
    let run = cli(&mut s, "S1", "show running-config");
    assert!(!run.contains("cos-dscp") && !run.contains("mls qos cos"), "{run}");
    assert!(cli(&mut s, "S1", "show mls qos maps").contains("default"));
    // Bad values are refused.
    cli(&mut s, "S1", "configure terminal");
    assert!(cli(&mut s, "S1", "mls qos map cos-dscp 1 2 3").contains("eight"));
    assert!(cli(&mut s, "S1", "mls qos map cos-dscp 0 8 16 24 32 99 48 56").contains("not a DSCP"));
    cli(&mut s, "S1", "interface fa0/1");
    assert!(cli(&mut s, "S1", "mls qos cos 8").contains("0 to 7"));
    assert!(cli(&mut s, "S1", "switchport priority extend cos x").contains("0 to 7"));
}
