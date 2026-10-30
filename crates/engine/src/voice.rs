//! IP phone commands: the dial pad (`phone.dial`, `phone.answer`, `phone.hangup`) and what the
//! phone's screen shows (`phone.status`).

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
        "protocol": if dev.voice.sip { "sip" } else { "own" },
        "sip_server": dev.voice.sip_server.map(|a| a.to_string()),
        "voice_vlan": netlabcraft_sim::voice::voice_vlan(&s.topology, d),
        "registered": st.number.is_some(),
        "number": st.number,
        "server": st.agent.map(|a| a.to_string()),
        "message": st.message,
        "power": crate::families::power_source(s, d),
        "call": call,
    })
}

pub(crate) fn run(s: &mut Session, cmd: &str, p: &Value) -> Result<Value> {
    let d = crate::cmd::device_param(s, p, "device")?;
    if s.topology.device(d).is_none_or(|x| x.kind != DeviceKind::Phone) {
        return Err(bad("phone commands run on IP phones"));
    }
    let r = match cmd {
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
