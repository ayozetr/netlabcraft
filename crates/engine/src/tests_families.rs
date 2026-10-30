//! Engine tests for the device families: the switch card in routers, power over Ethernet,
//! SIP phones, IoT things on a timer, the cloud and the modems.

use serde_json::json;

use crate::Session;

fn cli(s: &mut Session, dev: &str, line: &str) -> String {
    s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
}

fn marks(s: &mut Session, from: &str, to: &str) -> String {
    s.execute("net.ping", json!({"from": from, "to": to, "count": 4})).unwrap();
    s.execute("net.ping", json!({"from": from, "to": to, "count": 3})).unwrap()["marks"].as_str().unwrap_or_default().to_string()
}

/// Saving and opening the lab gives the same lab back.
fn round_trip(s: &mut Session) -> Session {
    let saved = s.execute("file.save", json!({})).unwrap();
    let mut s2 = Session::new();
    s2.execute("file.open", json!({"json": saved["json"]})).unwrap();
    assert_eq!(s2.topology, s.topology);
    s2
}

#[test]
fn the_switch_card_sample_switches_routes_and_survives_save_and_power() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "switch-card"})).unwrap();
    assert_eq!(marks(&mut s, "PC1", "192.168.10.11"), "!!!", "switched over the trunk");
    assert_eq!(marks(&mut s, "PC1", "192.168.20.11"), "!!!", "routed by the VLAN interfaces");
    let mut s2 = round_trip(&mut s);
    assert_eq!(marks(&mut s2, "PC2", "192.168.10.10"), "!!!");
    // A power cycle boots from the saved configuration; the VLAN database stays.
    s2.execute("device.power", json!({"device": "R1", "on": false})).unwrap();
    s2.execute("device.power", json!({"device": "R1", "on": true})).unwrap();
    s2.execute("sim.advance", json!({"seconds": 35})).unwrap();
    assert!(cli(&mut s2, "R1", "show vlan brief").contains("STAFF"));
    assert_eq!(marks(&mut s2, "PC1", "192.168.20.10"), "!!!");
    // The card comes out (switched off): its cables are unplugged.
    s2.execute("device.power", json!({"device": "R1", "on": false})).unwrap();
    let links = s2.topology.links.len();
    s2.execute("device.module.remove", json!({"device": "R1", "slot": "0/1"})).unwrap();
    assert_eq!(s2.topology.links.len(), links - 3);
    assert!(!s2.topology.find("R1").unwrap().has_switch_card());
}

#[test]
fn phones_boot_on_poe_or_their_adapter_and_stay_dark_on_a_plain_switch() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "ip-phones"})).unwrap();
    let st = s.execute("phone.status", json!({"device": "Phone1"})).unwrap();
    assert_eq!((st["registered"].clone(), st["power"]["source"].clone()), (json!(true), json!("poe")), "{st}");
    assert!(cli(&mut s, "S1", "show power inline").contains("Phone1"));
    // `power inline never` darkens the phone: it forgets its registration.
    for l in ["enable", "configure terminal", "interface fa0/1", "power inline never", "end"] {
        cli(&mut s, "S1", l);
    }
    let src = s.execute("device.power.source", json!({"device": "Phone1"})).unwrap();
    assert_eq!(src["source"], "none", "{src}");
    assert!(src["detail"].as_str().unwrap().contains("power inline never"));
    s.execute("sim.advance", json!({"seconds": 2})).unwrap();
    assert!(s.execute("phone.dial", json!({"device": "Phone1", "number": "1002"})).is_err(), "a dark phone can't dial");
    // Its adapter plugged in, it runs again (and the file keeps the adapter).
    s.execute("device.adapter", json!({"device": "Phone1", "on": true})).unwrap();
    let st = s.execute("phone.status", json!({"device": "Phone1"})).unwrap();
    assert_eq!((st["registered"].clone(), st["power"]["source"].clone()), (json!(true), json!("adapter")), "{st}");
    let s2 = round_trip(&mut s);
    assert_eq!(s2.topology.find("Phone1").unwrap().adapter, Some(true));
    assert!(s2.topology.find("S1").unwrap().ports[0].poe_never);
    // A phone on a plain switch stays dark and says why.
    let mut s = Session::new();
    s.execute("device.add", json!({"model": "Switch 24FE", "name": "SW"})).unwrap();
    s.execute("device.add", json!({"model": "IP Phone", "name": "P"})).unwrap();
    s.execute("link.add", json!({"a": "P", "a_port": "fa0", "b": "SW", "b_port": "fa0/1"})).unwrap();
    let src = s.execute("device.power.source", json!({"device": "P"})).unwrap();
    assert!(src["detail"].as_str().unwrap().contains("gives no power over Ethernet"), "{src}");
    assert!(s.execute("device.adapter", json!({"device": "SW", "on": true})).is_err(), "a switch has no adapter to plug");
    assert!(s.execute("device.adapter", json!({"device": "P"})).is_err());
}

#[test]
fn sip_phones_register_ring_talk_and_hang_up_and_captures_show_sip() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sip-phones"})).unwrap();
    let p1 = s.execute("phone.status", json!({"device": "Phone1"})).unwrap();
    let p2 = s.execute("phone.status", json!({"device": "Phone2"})).unwrap();
    assert_eq!((p1["registered"].clone(), p1["number"].clone(), p1["protocol"].clone()), (json!(true), json!("2001"), json!("sip")), "{p1}");
    assert_eq!(p2["number"], "2002", "{p2}");
    assert!(cli(&mut s, "R1", "show voice register all").contains("2002         idle (registered)"));
    // Unknown numbers: 404 from the router.
    let r = s.execute("phone.dial", json!({"device": "Phone1", "number": "2999"})).unwrap();
    assert!(r["call"].is_null() && r["message"].as_str().unwrap().contains("404"), "{r}");
    let r = s.execute("phone.dial", json!({"device": "Phone1", "number": "2002"})).unwrap();
    assert_eq!(r["call"]["state"], "ringback", "{r}");
    assert_eq!(s.execute("phone.status", json!({"device": "Phone2"})).unwrap()["call"]["state"], "ringing");
    let r = s.execute("phone.answer", json!({"device": "Phone2"})).unwrap();
    assert_eq!(r["call"]["state"], "connected", "{r}");
    s.execute("sim.advance", json!({"seconds": 3})).unwrap();
    let r = s.execute("phone.status", json!({"device": "Phone1"})).unwrap();
    assert_eq!(r["call"]["state"], "connected", "{r}");
    assert!(r["call"]["rtp_received"].as_u64().unwrap() >= 1, "{r}");
    assert!(cli(&mut s, "R1", "show voice register all").contains("in a call"));
    let r = s.execute("phone.hangup", json!({"device": "Phone1"})).unwrap();
    assert!(r["call"].is_null(), "{r}");
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    assert!(s.execute("phone.status", json!({"device": "Phone2"})).unwrap()["call"].is_null());
    // The event list shows the SIP exchange, in its own words.
    let summaries: Vec<String> = s.sim.trace().iter().map(|e| e.summary.clone()).collect();
    for want in [
        "SIP REGISTER sip:",
        "SIP 200 OK (REGISTER)",
        "SIP INVITE sip:2002@",
        "SIP 100 Trying",
        "SIP 180 Ringing (INVITE)",
        "SIP 200 OK (INVITE)",
        "SIP ACK",
        "SIP BYE",
        "SIP 404 Not Found",
    ] {
        assert!(summaries.iter().any(|x| x.contains(want)), "no `{want}` in the event list");
    }
    // The capture holds the message text as sent.
    let pcap = s.sim.pcap();
    assert!(pcap.windows(28).any(|w| w == b"INVITE sip:2002@192.168.50.1"), "the capture carries the SIP text");
    // A declined call ends with 603 at the caller.
    s.execute("phone.dial", json!({"device": "Phone2", "number": "2001"})).unwrap();
    s.execute("phone.hangup", json!({"device": "Phone1"})).unwrap();
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    let r = s.execute("phone.status", json!({"device": "Phone2"})).unwrap();
    assert!(r["call"].is_null() && r["message"].as_str().unwrap().contains("603"), "{r}");
    // Back to the own signalling: no voice register for it on R1, so it can't register.
    let r = s.execute("phone.configure", json!({"device": "Phone1", "protocol": "own"})).unwrap();
    assert_eq!((r["protocol"].clone(), r["registered"].clone()), (json!("own"), json!(false)), "{r}");
    assert!(s.execute("phone.configure", json!({"device": "Phone1", "protocol": "h323"})).is_err());
    assert!(s.execute("phone.configure", json!({"device": "R1", "protocol": "sip"})).is_err());
    let s2 = round_trip(&mut s);
    assert!(s2.topology.find("Phone2").unwrap().voice.sip);
    assert_eq!(s2.topology.find("R1").unwrap().voice.pools.len(), 2);
}

#[test]
fn a_smart_home_reports_on_a_timer_and_its_gateway_applies_and_or_rules() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "smart-home"})).unwrap();
    let page = |s: &mut Session| s.execute("iot.server", json!({"device": "Home"})).unwrap();
    let thing = |p: &serde_json::Value, n: &str| p["things"].as_array().unwrap().iter().find(|t| t["name"] == n).cloned().unwrap_or_default();
    let p = page(&mut s);
    assert_eq!(p["things"].as_array().unwrap().len(), 8, "{p}");
    assert_eq!(thing(&p, "Temp1")["what"], "temperature");
    assert_eq!(thing(&p, "Light1")["on"], false);
    // Someone moves: the hall light and the camera switch on; the camera takes a still image.
    s.execute("iot.configure", json!({"device": "Motion1", "value": 1})).unwrap();
    let p = page(&mut s);
    assert_eq!((thing(&p, "Light1")["on"].clone(), thing(&p, "Cam1")["on"].clone()), (json!(true), json!(true)), "{p}");
    let cam = s.execute("iot.configure", json!({"device": "Cam1"})).unwrap();
    assert!(cam["image"].as_str().unwrap_or_default().contains("someone in view"), "{cam}");
    // Smoke: the siren sounds, the door unlocks (OR), the camera stops (AND with smoke < 20).
    s.execute("iot.configure", json!({"device": "Smoke1", "value": 40})).unwrap();
    let p = page(&mut s);
    for (n, on) in [("Siren1", true), ("Door1", true), ("Cam1", false)] {
        assert_eq!(thing(&p, n)["on"], on, "{n}: {p}");
    }
    s.execute("iot.configure", json!({"device": "Smoke1", "value": 0})).unwrap();
    s.execute("iot.configure", json!({"device": "Temp1", "value": 70})).unwrap();
    let p = page(&mut s);
    assert_eq!(
        (thing(&p, "Door1")["on"].clone(), thing(&p, "Siren1")["on"].clone(), thing(&p, "Fan1")["on"].clone()),
        (json!(true), json!(false), json!(true))
    );
    // The timer: a reading changed behind the server's back reaches it on the next report.
    let id = s.topology.resolve("Temp1").unwrap();
    s.topology.device_mut(id).unwrap().iot.value = 12;
    s.execute("sim.advance", json!({"seconds": 6})).unwrap();
    let p = s.execute("iot.sync", json!({})).unwrap();
    assert!(p["things"].as_array().unwrap().iter().any(|t| t["device"] == "Temp1" && t["interval"] == 5));
    assert_eq!(thing(&page(&mut s), "Temp1")["value"], 12);
    // With no timer, only changes and asked-for reports reach it.
    s.execute("iot.configure", json!({"device": "Temp1", "interval": 0})).unwrap();
    s.topology.device_mut(id).unwrap().iot.value = 33;
    s.execute("sim.advance", json!({"seconds": 20})).unwrap();
    let reg = s.sim.iot_registry(s.topology.resolve("Home").unwrap()).iter().find(|e| e.name == "Temp1").and_then(|e| e.value);
    assert_eq!(reg, Some(12));
    // Rules in the new form, and hostile ones.
    let r = s
        .execute(
            "iot.server",
            json!({"device": "Home", "rules": [{"conditions": [{"sensor": "Temp1", "op": "<", "value": 15}, {"sensor": "Motion1", "op": "=", "value": 1}], "join": "and", "actuator": "Fan1", "on": false}]}),
        )
        .unwrap();
    assert_eq!(r["rules"][0]["text"], "If Temp1 < 15 and Motion1 = 1, switch Fan1 off", "{r}");
    for bad in [
        json!([{"conditions": [{"sensor": "x", "op": "!", "value": 1}], "actuator": "y"}]),
        json!([{"conditions": [{"sensor": "x", "value": 1}], "join": "xor", "actuator": "y"}]),
        json!([{"conditions": [], "actuator": "y"}]),
        json!([{"conditions": "x", "actuator": "y"}]),
    ] {
        assert!(s.execute("iot.server", json!({"device": "Home", "rules": bad})).is_err());
    }
    assert!(s.execute("iot.configure", json!({"device": "Light1", "value": 3})).is_err(), "a light has no reading");
    assert!(s.execute("iot.configure", json!({"device": "Temp1", "interval": 99_999})).is_err());
    let s2 = round_trip(&mut s);
    assert!(s2.topology.find("Home").unwrap().iot.service);
    assert_eq!(s2.topology.find("Motion1").unwrap().iot.interval, 5);
}
