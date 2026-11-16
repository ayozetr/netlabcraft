//! IP phone commands: the dial pad (`phone.dial`, `phone.answer`, `phone.hangup`) and what the
//! phone's screen shows (`phone.status`); and the same for the Softphone app of a PC, laptop or
//! server (`softphone.config`, `softphone.status`, `softphone.dial`, `softphone.answer`,
//! `softphone.hangup`), which registers and calls through the same call-control machinery.

use netlabcraft_model::DeviceKind;
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams(msg.into())
}

/// What a phone's screen shows.
pub(crate) fn status(s: &Session, d: netlabcraft_core::DeviceId) -> Value {
    let Some(dev) = s.topology.device(d) else { return Value::Null };
    let st = s.sim.phone(d).cloned().unwrap_or_default();
    let call = st.call.as_ref().map(|c| {
        json!({"state": c.state.name(), "peer": c.peer, "peer_address": c.peer_ip.map(|a| a.to_string()), "rtp_sent": c.rtp_sent, "rtp_received": c.rtp_received})
    });
    json!({
        "device": dev.name,
        "address": dev.ports.first().and_then(|p| p.ip).map(|n| n.to_string()),
        "tftp": dev.voice.tftp.map(|a| a.to_string()),
        "protocol": if dev.voice_sip() { "sip" } else { "own" },
        "sip_server": dev.voice.sip_server.map(|a| a.to_string()),
        "softphone": dev.voice.softphone.is_some(),
        "call_manager": dev.voice.softphone.as_ref().and_then(|x| x.server).map(|a| a.to_string()),
        "mac": dev.ports.first().map(|p| p.mac.to_string()),
        "voice_vlan": netlabcraft_sim::voice::voice_vlan(&s.topology, d),
        "registered": st.number.is_some(),
        "number": st.number,
        "server": st.agent.map(|a| a.to_string()),
        "message": st.message,
        "power": crate::families::power_source(s, d),
        "call": call,
    })
}

/// `softphone.config`: switches the Softphone app on or off and sets its call manager and protocol.
fn config(s: &mut Session, d: netlabcraft_core::DeviceId, p: &Value) -> Result<Value> {
    let enabled = match p.get("enabled") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| bad("`enabled` is true or false"))?),
    };
    let sip = match p.get("protocol").and_then(Value::as_str) {
        None => None,
        Some(v) if v.eq_ignore_ascii_case("sip") => Some(true),
        Some(v) if v.eq_ignore_ascii_case("own") || v.eq_ignore_ascii_case("netlabcraft") => Some(false),
        Some(v) => return Err(bad(format!("`protocol` is \"sip\" or \"own\", not `{v}`"))),
    };
    // `server`: an address, or empty / null for the TFTP server DHCP option 150 gave.
    let server: Option<Option<std::net::Ipv4Addr>> = match p.get("server") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => {
            let text = v.as_str().ok_or_else(|| bad("`server` is the call manager's address (empty: the one DHCP gave)"))?.trim();
            if text.is_empty() { Some(None) } else { Some(Some(text.parse().map_err(|_| bad(format!("`{text}` is not an IPv4 address")))?)) }
        }
    };
    if enabled.is_none() && sip.is_none() && server.is_none() {
        return Err(bad("give `enabled`, `protocol` (\"sip\" or \"own\") and/or `server` (the call manager)"));
    }
    let on_now = s.topology.device(d).is_some_and(|x| x.voice.softphone.is_some());
    if !on_now && enabled.is_none() {
        return Err(EngineError::Failed("the Softphone is off: switch it on first (`enabled`: true)".into()));
    }
    s.checkpoint();
    if let Some(dev) = s.topology.device_mut(d) {
        match enabled {
            Some(false) => dev.voice.softphone = None,
            Some(true) if dev.voice.softphone.is_none() => dev.voice.softphone = Some(netlabcraft_model::voice::Softphone::default()),
            _ => {}
        }
        if let Some(sp) = dev.voice.softphone.as_mut() {
            if let Some(v) = sip {
                sp.sip = v;
            }
            if let Some(a) = server {
                sp.server = a;
            }
        }
    }
    // It registers again with its new settings.
    s.sim.reset_voice(d);
    s.changed();
    s.sim.phone_sync(&s.topology);
    Ok(status(s, d))
}

pub(crate) fn run(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    let d = crate::cmd::device_param(s, p, "device")?;
    let softphone = cmd.starts_with("softphone.");
    let Some(dev) = s.topology.device(d) else { return Err(bad("no such device")) };
    if softphone {
        if !dev.can_softphone() {
            return Err(bad("the Softphone runs on PCs, laptops and servers"));
        }
        if cmd == "softphone.config" {
            return config(s, d, p);
        }
        if dev.voice.softphone.is_none() {
            return Err(EngineError::Failed(format!("the Softphone of {} is off: switch it on with softphone.config", dev.name)));
        }
    } else if dev.kind != DeviceKind::Phone {
        return Err(bad("phone commands run on IP phones"));
    }
    let r = match cmd.replacen("softphone.", "phone.", 1).as_str() {
        "phone.status" => {
            // Let the phone register if it can.
            s.sim.phone_sync(&s.topology);
            Ok(())
        }
        "phone.dial" => {
            let number = p.get("number").and_then(Value::as_str).ok_or_else(|| bad("`number` (digits) is required"))?.trim().to_string();
            s.sim.phone_dial(&s.topology, d, &number)
        }
        "phone.answer" => s.sim.phone_answer(&s.topology, d, false),
        "phone.hangup" => s.sim.phone_answer(&s.topology, d, true),
        other => return Err(EngineError::UnknownCommand(other.to_string())),
    };
    s.revision += 1;
    r.map_err(|e| EngineError::Failed(e.to_string()))?;
    Ok(status(s, d))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn cli(s: &mut Session, dev: &str, line: &str) -> String {
        s.execute("cli.exec", json!({"device": dev, "line": line})).unwrap()["output"].as_str().unwrap_or_default().to_string()
    }

    #[test]
    fn two_phones_register_ring_talk_and_hang_up() {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "ip-phones"})).unwrap();
        let ip = |s: &Session, n: &str| s.topology.find(n).unwrap().ports[0].ip.unwrap().addr.to_string();
        assert!(ip(&s, "Phone1").starts_with("192.168.100."), "the phone is in the voice VLAN");
        assert!(ip(&s, "PC1").starts_with("192.168.10."), "the PC behind it is in the data VLAN");
        let p1 = s.execute("phone.status", json!({"device": "Phone1"})).unwrap();
        let p2 = s.execute("phone.status", json!({"device": "Phone2"})).unwrap();
        assert_eq!(
            (p1["registered"].clone(), p1["voice_vlan"].clone(), p1["tftp"].clone()),
            (json!(true), json!(100), json!("192.168.100.1")),
            "{p1}"
        );
        assert_eq!(p2["registered"], true, "{p2}");
        let n2 = p2["number"].as_str().unwrap().to_string();
        assert_ne!(p1["number"].as_str().unwrap(), n2);
        // Unknown numbers are refused.
        let r = s.execute("phone.dial", json!({"device": "Phone1", "number": "4444"})).unwrap();
        assert!(r["call"].is_null() && r["message"].as_str().unwrap().contains("unknown"), "{r}");
        assert!(s.execute("phone.dial", json!({"device": "Phone1", "number": "12ab"})).is_err());
        // Ring, answer, talk.
        let r = s.execute("phone.dial", json!({"device": "Phone1", "number": n2})).unwrap();
        assert_eq!(r["call"]["state"], "ringback", "{r}");
        let r = s.execute("phone.status", json!({"device": "Phone2"})).unwrap();
        assert_eq!(r["call"]["state"], "ringing", "{r}");
        assert!(s.execute("phone.answer", json!({"device": "Phone1"})).is_err(), "the caller doesn't answer");
        let r = s.execute("phone.answer", json!({"device": "Phone2"})).unwrap();
        assert_eq!(r["call"]["state"], "connected", "{r}");
        s.execute("sim.advance", json!({"seconds": 3})).unwrap();
        let r = s.execute("phone.status", json!({"device": "Phone1"})).unwrap();
        assert!(r["call"]["rtp_received"].as_u64().unwrap() >= 1 && r["call"]["rtp_sent"].as_u64().unwrap() >= 1, "{r}");
        assert!(cli(&mut s, "R1", "show ephone-dn").contains("in a call"));
        let eph = cli(&mut s, "R1", "show ephone");
        assert!(eph.contains("registered (auto assign)"), "{eph}");
        // Voice frames crossed the switch tagged in VLAN 100.
        assert!(s.sim.trace().iter().any(|e| e.summary.contains(":16384") && e.summary.contains("VLAN 100")), "voice packets tagged");
        let r = s.execute("phone.hangup", json!({"device": "Phone2"})).unwrap();
        assert!(r["call"].is_null(), "{r}");
        assert!(s.execute("phone.status", json!({"device": "Phone1"})).unwrap()["call"].is_null());
        // The configurations read back.
        cli(&mut s, "R1", "enable");
        cli(&mut s, "S1", "enable");
        let run = cli(&mut s, "R1", "show running-config");
        for l in ["telephony-service", " auto assign 1 to 4", "ephone-dn 2", " number 1002", " option 150 ip 192.168.100.1"] {
            assert!(run.contains(l), "{l}\n{run}");
        }
        assert!(cli(&mut s, "S1", "show running-config").contains(" switchport voice vlan 100"));
        assert!(s.execute("phone.dial", json!({"device": "R1", "number": "1"})).is_err());
    }

    fn cli_lines(s: &mut Session, dev: &str, text: &str) {
        for l in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let out = cli(s, dev, l);
            assert!(!out.starts_with('%'), "{l}: {out}");
        }
    }

    #[test]
    fn a_pcs_softphone_registers_calls_ip_phones_and_other_softphones() {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "ip-phones"})).unwrap();
        for ph in ["Phone1", "Phone2"] {
            s.execute("phone.status", json!({"device": ph})).unwrap();
        }
        // Off until it is switched on, and only for computers.
        assert!(s.execute("softphone.status", json!({"device": "PC1"})).is_err());
        assert!(s.execute("softphone.dial", json!({"device": "PC1", "number": "1002"})).is_err());
        assert!(s.execute("softphone.config", json!({"device": "Phone1", "enabled": true})).is_err(), "an IP phone is not a computer");
        assert!(s.execute("softphone.config", json!({"device": "R1", "enabled": true})).is_err());
        assert!(s.execute("softphone.config", json!({"device": "PC1", "server": "192.168.100.1"})).is_err(), "off: switch it on first");
        assert!(s.execute("softphone.config", json!({"device": "PC1"})).is_err());
        assert!(s.execute("softphone.config", json!({"device": "PC1", "enabled": true, "protocol": "h323"})).is_err());
        assert!(s.execute("softphone.config", json!({"device": "PC1", "enabled": true, "server": "not-an-address"})).is_err());
        assert!(s.topology.find("PC1").unwrap().voice.softphone.is_none(), "a refused command changes nothing");
        // Two more directory numbers on the call manager (auto assign covers tags 1 to 4).
        cli_lines(&mut s, "R1", "enable\nconfigure terminal\nephone-dn 3\nnumber 1003\nephone-dn 4\nnumber 1004\nend");
        for pc in ["PC1", "PC2"] {
            let r = s.execute("softphone.config", json!({"device": pc, "enabled": true, "server": "192.168.100.1"})).unwrap();
            assert_eq!((r["softphone"].clone(), r["protocol"].clone()), (json!(true), json!("own")), "{r}");
        }
        let st = |s: &mut Session, d: &str| s.execute("softphone.status", json!({"device": d})).unwrap();
        let (p1, p2) = (st(&mut s, "PC1"), st(&mut s, "PC2"));
        assert_eq!((p1["registered"].clone(), p2["registered"].clone()), (json!(true), json!(true)), "{p1} {p2}");
        let (n1, n2) = (p1["number"].as_str().unwrap().to_string(), p2["number"].as_str().unwrap().to_string());
        assert!(n1.starts_with("100") && n2.starts_with("100") && n1 != n2, "{p1} {p2}");
        assert_eq!(p1["call_manager"], "192.168.100.1");
        // The call manager lists the softphones as registered endpoints.
        let eph = cli(&mut s, "R1", "show ephone");
        let mac = s.topology.find("PC1").unwrap().ports[0].mac.to_string();
        assert!(eph.contains(&mac) && eph.contains(&n1), "{eph}");
        assert!(cli(&mut s, "R1", "show ephone-dn").contains("idle (registered)"));
        // A softphone calls an IP phone: ring, answer, talk.
        let phone2 = s.execute("phone.status", json!({"device": "Phone2"})).unwrap()["number"].as_str().unwrap().to_string();
        let r = s.execute("softphone.dial", json!({"device": "PC1", "number": phone2})).unwrap();
        assert_eq!(r["call"]["state"], "ringback", "{r}");
        assert_eq!(s.execute("phone.status", json!({"device": "Phone2"})).unwrap()["call"]["state"], "ringing");
        assert!(s.execute("softphone.answer", json!({"device": "PC1"})).is_err(), "the caller does not answer");
        assert!(s.execute("softphone.dial", json!({"device": "PC1", "number": "1004"})).is_err(), "already in a call");
        let r = s.execute("phone.answer", json!({"device": "Phone2"})).unwrap();
        assert_eq!(r["call"]["state"], "connected", "{r}");
        s.execute("sim.advance", json!({"seconds": 3})).unwrap();
        let r = st(&mut s, "PC1");
        assert_eq!(r["call"]["state"], "connected", "{r}");
        assert!(r["call"]["rtp_sent"].as_u64().unwrap() >= 1 && r["call"]["rtp_received"].as_u64().unwrap() >= 1, "{r}");
        assert!(cli(&mut s, "R1", "show ephone-dn").contains("in a call"));
        // The softphone's voice leaves its port marked DSCP EF; the frames are in the event list.
        let pc1 = s.topology.find("PC1").unwrap().id;
        let marked = s
            .sim
            .trace()
            .iter()
            .filter(|e| e.from.device == pc1 && e.summary.contains(":16384") && e.frame.get(12..14) == Some(&[0x08, 0x00][..]))
            .map(|e| e.frame.get(15).copied().unwrap_or(0) >> 2)
            .collect::<Vec<_>>();
        assert!(marked.contains(&46), "voice leaves the softphone marked DSCP EF: {marked:?}");
        let r = s.execute("softphone.hangup", json!({"device": "PC1"})).unwrap();
        assert!(r["call"].is_null(), "{r}");
        assert!(s.execute("phone.status", json!({"device": "Phone2"})).unwrap()["call"].is_null());
        // Two softphones call each other; the callee declines.
        let r = s.execute("softphone.dial", json!({"device": "PC2", "number": n1})).unwrap();
        assert_eq!(r["call"]["state"], "ringback", "{r}");
        assert_eq!(st(&mut s, "PC1")["call"]["state"], "ringing");
        s.execute("softphone.answer", json!({"device": "PC1"})).unwrap();
        s.execute("sim.advance", json!({"seconds": 2})).unwrap();
        assert!(st(&mut s, "PC2")["call"]["rtp_received"].as_u64().unwrap() >= 1);
        s.execute("softphone.hangup", json!({"device": "PC2"})).unwrap();
        assert!(st(&mut s, "PC1")["call"].is_null());
        // An IP phone calls a softphone.
        let r = s.execute("phone.dial", json!({"device": "Phone2", "number": n1})).unwrap();
        assert_eq!(r["call"]["state"], "ringback", "{r}");
        assert_eq!(st(&mut s, "PC1")["call"]["state"], "ringing");
        s.execute("softphone.hangup", json!({"device": "PC1"})).unwrap();
        assert!(s.execute("phone.status", json!({"device": "Phone2"})).unwrap()["call"].is_null());
        // The settings are kept in the lab file; switching it off frees the extension.
        let back = netlabcraft_model::file::from_json(&netlabcraft_model::file::to_json(&s.topology).unwrap()).unwrap();
        assert_eq!(back.find("PC1").unwrap().voice.softphone, s.topology.find("PC1").unwrap().voice.softphone);
        let r = s.execute("softphone.config", json!({"device": "PC1", "enabled": false})).unwrap();
        assert_eq!(r["softphone"], false, "{r}");
        assert!(s.topology.find("PC1").unwrap().voice.softphone.is_none());
        s.execute("sim.advance", json!({"seconds": 1})).unwrap();
        assert!(cli(&mut s, "R1", "show ephone-dn").contains("idle (registered)"), "the others stay registered");
        assert!(s.execute("softphone.dial", json!({"device": "PC1", "number": "1002"})).is_err());
    }

    #[test]
    fn a_softphone_takes_its_call_manager_from_dhcp_and_can_signal_with_sip() {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "sip-phones"})).unwrap();
        // A PC on the phones' switch, with the SIP number its MAC address names.
        s.execute("device.add", json!({"kind": "pc", "name": "SoftPC"})).unwrap();
        s.execute("link.add", json!({"a": "SoftPC", "a_port": "fa0", "b": "S1", "b_port": "fa0/10"})).unwrap();
        s.execute("cli.exec", json!({"device": "SoftPC", "line": "ipconfig /renew"})).unwrap();
        let mac = s.topology.find("SoftPC").unwrap().ports[0].mac.to_string();
        cli_lines(
            &mut s,
            "R1",
            &format!("enable\nconfigure terminal\nvoice register dn 3\nnumber 2003\nexit\nvoice register pool 3\nid mac {mac}\nnumber 1 dn 3\nend"),
        );
        let r = s.execute("softphone.config", json!({"device": "SoftPC", "enabled": true, "protocol": "sip"})).unwrap();
        assert_eq!(r["protocol"], "sip", "{r}");
        assert!(r["call_manager"].is_null(), "no address typed: DHCP option 150 is used");
        let r = s.execute("softphone.status", json!({"device": "SoftPC"})).unwrap();
        assert_eq!((r["registered"].clone(), r["number"].clone()), (json!(true), json!("2003")), "{r}");
        assert!(cli(&mut s, "R1", "show voice register all").contains("2003         idle (registered)"));
        let r = s.execute("softphone.dial", json!({"device": "SoftPC", "number": "2001"})).unwrap();
        assert_eq!(r["call"]["state"], "ringback", "{r}");
        s.execute("phone.answer", json!({"device": "Phone1"})).unwrap();
        s.execute("sim.advance", json!({"seconds": 3})).unwrap();
        let r = s.execute("softphone.status", json!({"device": "SoftPC"})).unwrap();
        assert_eq!(r["call"]["state"], "connected", "{r}");
        assert!(r["call"]["rtp_received"].as_u64().unwrap() >= 1, "{r}");
        assert!(s.sim.trace().iter().any(|e| e.summary.contains("SIP INVITE sip:2001@")));
    }

    #[test]
    fn configured_ephones_take_their_number_by_mac() {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "ip-phones"})).unwrap();
        let mac = s.topology.find("Phone2").unwrap().ports[0].mac.to_string();
        for l in [
            "enable",
            "configure terminal",
            "telephony-service",
            "no auto assign",
            "exit",
            "ephone 1",
            &format!("mac-address {mac}"),
            "button 1:1",
            "end",
        ] {
            let out = cli(&mut s, "R1", l);
            assert!(!out.starts_with('%'), "{l}: {out}");
        }
        s.execute("sim.reset", json!({})).unwrap();
        let p2 = s.execute("phone.status", json!({"device": "Phone2"})).unwrap();
        assert_eq!(p2["number"], "1001", "{p2}");
        let p1 = s.execute("phone.status", json!({"device": "Phone1"})).unwrap();
        assert_eq!(p1["registered"], false);
        assert!(p1["message"].as_str().unwrap_or_default().contains("refused"), "{p1}");
        cli(&mut s, "R1", "configure terminal");
        assert!(cli(&mut s, "R1", "number 5").contains("% "), "number belongs under ephone-dn");
    }
}
