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

#[test]
fn home_internet_crosses_cable_and_dsl_modems_through_the_cloud() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "home-internet"})).unwrap();
    assert_eq!(marks(&mut s, "PC1", "10.0.0.10"), "!!!", "home PC to the office server");
    // The cloud's pairs: parting coaxial from phone cuts the path.
    let r = s.execute("cloud.map", json!({"device": "ISP"})).unwrap();
    assert!(r["pairs"].as_array().unwrap().iter().any(|p| p == &json!(["Coaxial0", "Phone0"])), "{r}");
    s.execute("cloud.map", json!({"device": "ISP", "a": "Coaxial0", "b": "Phone0", "remove": true})).unwrap();
    assert_ne!(marks(&mut s, "PC1", "10.0.0.10"), "!!!");
    s.execute("cloud.map", json!({"device": "ISP", "a": "co0", "b": "ph0"})).unwrap();
    assert_eq!(marks(&mut s, "PC1", "10.0.0.10"), "!!!");
    assert!(s.execute("cloud.map", json!({"device": "ISP", "a": "co0"})).is_err());
    assert!(s.execute("cloud.map", json!({"device": "Office", "a": "g0/0", "b": "g0/1"})).is_err());
    // Coaxial and phone ports take only their own cable.
    s.execute("device.add", json!({"model": "Cable Modem", "name": "CM2"})).unwrap();
    s.execute("device.add", json!({"model": "DSL Modem", "name": "DM2"})).unwrap();
    assert!(s.execute("link.add", json!({"a": "CM2", "a_port": "Coaxial0", "b": "DM2", "b_port": "Phone0"})).is_err());
    assert!(s.execute("link.add", json!({"a": "CM2", "a_port": "Coaxial0", "b": "ISP", "b_port": "Coaxial1", "cable": "phone"})).is_err());
    let r = s.execute("link.add", json!({"a": "CM2", "a_port": "Coaxial0", "b": "ISP", "b_port": "Coaxial1"})).unwrap();
    assert_eq!(r["resolved"], "coaxial", "{r}");
    let r = s.execute("link.add", json!({"a": "DM2", "a_port": "Phone0", "b": "ISP", "b_port": "Phone1", "cable": "phone"})).unwrap();
    assert_eq!(r["resolved"], "phone", "{r}");
    assert!(s.execute("link.add", json!({"a": "DM2", "a_port": "Ethernet1", "b": "PC1", "cable": "coaxial"})).is_err());
    let s2 = round_trip(&mut s);
    assert_eq!(s2.topology.find("ISP").unwrap().bridges.len(), 3);
    assert!(s2.topology.links.iter().any(|l| l.cable == netlabcraft_model::cable::Cable::Phone));
}

#[test]
fn tablets_and_smartphones_join_wifi_take_dhcp_and_browse() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "mobile-devices"})).unwrap();
    for name in ["Tablet1", "Smartphone1"] {
        let d = s.topology.find(name).unwrap();
        assert!(d.dhcp_client && d.ports[0].ip.is_some(), "{name} took a lease");
        assert!(!d.has_rs232(), "no console port on a touch device");
        assert_eq!(marks(&mut s, name, "192.168.0.10"), "!!!", "{name} reaches the server over Wi-Fi");
    }
    let page = s.execute("browser.get", json!({"device": "Smartphone1", "url": "192.168.0.10"})).unwrap();
    assert!(page.to_string().contains("200"), "{page}");
    // Placed from the palette: named after the model, Wi-Fi only.
    let v = s.execute("device.add", json!({"model": "Smartphone", "x": 10, "y": 10})).unwrap();
    assert_eq!(v["name"], "Smartphone2");
    assert!(s.execute("device.add", json!({"model": "Tablet"})).is_ok());
    assert!(s.execute("pc.terminal", json!({"pc": "Tablet2"})).is_err(), "no RS-232 port for a Terminal");
    let s2 = round_trip(&mut s);
    assert_eq!(s2.topology.find("Smartphone2").unwrap().catalogue_model().id, "Smartphone");
}

#[test]
fn wifi_things_join_the_gateway_report_and_follow_the_rules() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "wifi-things"})).unwrap();
    let r = s.execute("iot.sync", json!({})).unwrap();
    let things = r["things"].as_array().unwrap();
    assert_eq!(things.len(), 10, "{r}");
    assert!(things.iter().all(|t| t["registered"] == "192.168.0.1"), "every thing registered over Wi-Fi: {r}");
    let on = |s: &mut Session, name: &str| s.execute("iot.configure", json!({"device": name})).unwrap()["on"].clone();
    assert_eq!((on(&mut s, "Heater1"), on(&mut s, "AC1")), (json!(false), json!(false)), "21 degrees: neither runs");
    s.execute("iot.configure", json!({"device": "Thermo1", "value": 15})).unwrap();
    assert_eq!(on(&mut s, "Heater1"), json!(true), "cold: the heater heats");
    s.execute("iot.configure", json!({"device": "Thermo1", "value": 30})).unwrap();
    assert_eq!((on(&mut s, "Heater1"), on(&mut s, "AC1")), (json!(false), json!(true)), "hot: the air conditioner cools");
    s.execute("iot.configure", json!({"device": "CO1", "value": 80})).unwrap();
    assert_eq!((on(&mut s, "Window1"), on(&mut s, "Garage1")), (json!(true), json!(true)), "CO opens the window and the garage");
    s.execute("iot.configure", json!({"device": "Soil1", "value": 10})).unwrap();
    assert_eq!(on(&mut s, "Sprinkler1"), json!(true));
    s.execute("iot.switch", json!({"device": "Home", "name": "Plug1", "on": true})).unwrap();
    assert_eq!(on(&mut s, "Plug1"), json!(true), "switched by hand");
    // Every new Wi-Fi thing looks for the home gateway's network when placed.
    let v = s.execute("device.add", json!({"model": "Garage Door", "x": 400, "y": 300})).unwrap();
    let g = s.topology.find(v["name"].as_str().unwrap()).unwrap();
    assert_eq!(g.ports[0].wifi.as_ref().map(|w| w.ssid.as_str()), Some("HomeNetwork"));
    let mut s2 = round_trip(&mut s);
    s2.execute("sim.advance", json!({"seconds": 6})).unwrap();
    assert!(s2.execute("iot.server", json!({"device": "Home"})).unwrap()["things"].as_array().unwrap().len() >= 10);
}

#[test]
fn a_board_program_reads_switches_reports_and_never_hangs() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "board"})).unwrap();
    s.execute("sim.advance", json!({"seconds": 1})).unwrap();
    let st = s.execute("board.status", json!({"device": "Board1"})).unwrap();
    assert_eq!(st["state"], "running", "{st}");
    let fan = s.topology.resolve("Fan1").unwrap();
    assert!(!s.sim.iot_thing(fan).is_some_and(|x| x.on), "22 degrees: the fan stays off");
    s.execute("iot.configure", json!({"device": "Temp1", "value": 35})).unwrap();
    s.execute("sim.advance", json!({"seconds": 3})).unwrap();
    assert!(s.sim.iot_thing(fan).is_some_and(|x| x.on), "35 degrees: the program switches the fan on");
    let st = s.execute("board.status", json!({"device": "Board1"})).unwrap();
    assert_eq!(st["vars"]["t"], 35, "{st}");
    assert!(st["log"].as_array().unwrap().iter().any(|l| l == "temperature 35"), "{st}");
    assert!(st["pins"].as_array().unwrap().iter().any(|p| p["pin"] == "A0" && p["wired"] == "Temp1" && p["level"] == 35), "{st}");
    let page = s.execute("iot.server", json!({"device": "Home"})).unwrap();
    assert!(page["things"].as_array().unwrap().iter().any(|t| t["name"] == "Board1" && t["value"] == 35), "the board reported: {page}");
    // Stop, a program that doesn't compile, one that never sleeps.
    let st = s.execute("board.program", json!({"device": "Board1", "run": false})).unwrap();
    assert_eq!(st["state"], "stopped");
    let err = s.execute("board.program", json!({"device": "Board1", "source": "if 1\nx = 2\n"})).unwrap_err().to_string();
    assert!(err.contains("without their `end`"), "{err}");
    let st = s.execute("board.program", json!({"device": "Board1", "source": "while 1\n  x = 1\nend\n", "run": true})).unwrap();
    assert!(st["message"].as_str().unwrap().contains("sleep"), "{st}");
    s.execute("sim.advance", json!({"seconds": 30})).unwrap();
    // IoT custom cables: by link.add (first free digital pin for a device), listed, removed.
    s.execute("device.add", json!({"model": "Siren", "name": "Siren1"})).unwrap();
    let l = s.execute("link.add", json!({"a": "Siren1", "b": "Board1", "cable": "iot-custom"})).unwrap();
    assert_eq!(l["a"], "Board1 D2", "{l}");
    assert!(s.execute("link.list", json!({})).unwrap().as_array().unwrap().iter().any(|x| x["a"] == "Board1 D2" && x["cable"] == "iot-custom"));
    assert!(s.execute("link.add", json!({"a": "Siren1", "b": "Board1", "cable": "iot-custom"})).is_err(), "wired once");
    assert!(s.execute("link.add", json!({"a": "Home", "b": "Siren1", "cable": "iot-custom"})).is_err(), "no board");
    s.execute("link.remove", json!({"device": "Board1", "port": "D2"})).unwrap();
    assert!(s.execute("board.wire", json!({"device": "Board1", "pin": "D9", "thing": "Siren1"})).is_err());
    s.execute("board.wire", json!({"device": "Board1", "pin": "d3", "thing": "Siren1"})).unwrap();
    let s2 = round_trip(&mut s);
    let b = s2.topology.find("Board1").unwrap();
    assert_eq!(b.board.wires.len(), 4);
    assert!(b.board.program.starts_with("while 1"));
    // Removing a thing unplugs its cable.
    let mut s3 = s2;
    s3.execute("device.remove", json!({"device": "Siren1"})).unwrap();
    assert_eq!(s3.topology.find("Board1").unwrap().board.wires.len(), 3);
}

#[test]
fn power_supplies_come_out_and_the_last_one_switches_the_device_off() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "two-subnets"})).unwrap();
    let r = s.execute("device.psu", json!({"device": "R1"})).unwrap();
    assert_eq!(r["bays"], json!([{"bay": 1, "fitted": true}, {"bay": 2, "fitted": false}]), "{r}");
    assert_eq!(marks(&mut s, "PC1", "10.0.2.10"), "!!!");
    // A spare in bay 2: the first can come out and the router keeps running.
    s.execute("device.psu", json!({"device": "R1", "bay": 2, "fitted": true})).unwrap();
    let r = s.execute("device.psu", json!({"device": "R1", "bay": 1, "fitted": false})).unwrap();
    assert_eq!(r["power"], "on");
    assert_eq!(marks(&mut s, "PC1", "10.0.2.10"), "!!!", "the spare keeps it up");
    // The last one out: off, and it can't be switched on without a supply.
    let r = s.execute("device.psu", json!({"device": "R1", "bay": 2, "fitted": false})).unwrap();
    assert_eq!((r["power"].clone(), r["no_supply"].clone()), (json!("off"), json!(true)));
    assert!(s.topology.find("R1").unwrap().powered_off);
    let err = s.execute("device.power", json!({"device": "R1", "on": true})).unwrap_err().to_string();
    assert!(err.contains("no power supply"), "{err}");
    s.execute("device.psu", json!({"device": "R1", "bay": 1, "fitted": true})).unwrap();
    s.execute("device.power", json!({"device": "R1", "on": true})).unwrap();
    assert!(!s.topology.find("R1").unwrap().powered_off);
    // Hostile: a bay that isn't there, a device without bays.
    assert!(s.execute("device.psu", json!({"device": "R1", "bay": 3, "fitted": true})).is_err());
    assert!(s.execute("device.psu", json!({"device": "PC1", "bay": 1, "fitted": false})).is_err());
    let s2 = round_trip(&mut s);
    assert_eq!(s2.topology.find("R1").unwrap().psus(), [true, false]);
}

#[test]
fn the_phone_settings_set_its_protocol_server_and_address() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "ip-phones"})).unwrap();
    let st = s.execute("phone.status", json!({"device": "Phone1"})).unwrap();
    assert!(st["voice_vlan"].as_u64().is_some() && st["number"].as_str().is_some(), "learned VLAN and extension: {st}");
    let st = s
        .execute("phone.configure", json!({"device": "Phone1", "address": "192.168.100.77", "mask": "255.255.255.0", "gateway": "192.168.100.1"}))
        .unwrap();
    assert_eq!(st["address"], "192.168.100.77/24", "{st}");
    assert!(!s.topology.find("Phone1").unwrap().dhcp_client);
    let st = s.execute("phone.configure", json!({"device": "Phone1", "address": "dhcp"})).unwrap();
    assert!(s.topology.find("Phone1").unwrap().dhcp_client, "{st}");
    assert!(s.execute("phone.configure", json!({"device": "Phone1", "address": "nonsense"})).is_err());
    assert!(s.execute("phone.configure", json!({"device": "Phone1", "address": "10.0.0.1", "mask": "x"})).is_err());
    assert!(s.execute("phone.configure", json!({"device": "PC1", "address": "dhcp"})).is_err());
}
