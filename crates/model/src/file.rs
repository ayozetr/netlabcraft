//! The `.nlcraft` lab file: UTF-8 JSON, `{"format":"netlabcraft","version":13,"topology":{…}}`.
//! Readers accept older versions and refuse newer ones; loading always sanitizes.

use serde::{Deserialize, Serialize};

use crate::{ModelError, Result, Topology};

pub const FORMAT: &str = "netlabcraft";
/// 2: the lab-level `environment` (the smart things' shared room).
/// 3: smartphones and tablets have a `Cellular0` port, and devices may carry `cell` settings.
/// 4: drawings have separate outline and fill settings (`outline`, `outline_color`, `outline_width`, `no_fill`).
/// 5: end devices may carry a `host_fw` (the Firewall app's IPv4 and IPv6 rule lists).
/// 6: end devices may carry a `pppoe` account and session, and servers a `pppoe` service in `services`.
/// 7: end devices may carry the VPN Client's settings (`vpn_client`), and routers a remote-access VPN server in `crypto.ra`.
/// 9: the physical section may carry furniture, passive devices (patch panels, wall mounts, power distribution devices), where things stand on furniture, patch wires and power plugs (`furniture`, `fittings`, `mounts`, `patch_wires`, `plugs`). Older files have none and open as before.
/// 8: devices may carry NetFlow settings (`netflow`: a router's export destinations, version and counting interfaces, an end device's collector port).
/// 10: two fiber cables, `fiber-single-mode` and `fiber-multi-mode` (an older `fiber` cable reads as multi-mode), a port's `single_mode` transceiver, USB cables (`usb` on a `consoles` entry) and octal cables (`octals`).
/// 11: the lab may carry `environments` (the Environments window: the environment clock and what each place sets). Older files have none.
/// 12: the lab may carry `images` (PNG and JPEG pictures, each stored once under the hash of its bytes), a `background` for the logical workspace, and `scenes` in the physical section (the background, icon and size of each place). Older files have none.
/// 13: the physical section may carry containers (a `container` place kind), inventory shelves, cable pegboards and industrial control panels (furniture kinds `inventory-shelf`, `pegboard`, `icp`), cables hung on pegs (`hangs`) and the working closet (`working`). Older files have none and open as before.
/// Older files still open (their phones get the port; a drawing with only `color` draws as it always did;
/// a device without `host_fw` has its firewall off; one without `pppoe` has no account and no session;
/// one without `vpn_client` or `crypto.ra` has no VPN client settings and no remote-access server;
/// one without `netflow` exports nothing and runs no collector).
pub const VERSION: u32 = 13;
pub const EXTENSION: &str = "nlcraft";

/// Largest lab file accepted (hostile input).
pub const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Envelope {
    format: String,
    version: u32,
    topology: Topology,
}

pub fn to_json(t: &Topology) -> Result<String> {
    let env = Envelope { format: FORMAT.into(), version: VERSION, topology: t.clone() };
    serde_json::to_string_pretty(&env).map_err(|e| ModelError::Invalid(format!("couldn't write the lab: {e}")))
}

pub fn from_json(s: &str) -> Result<Topology> {
    if s.len() > MAX_BYTES {
        return Err(ModelError::Invalid("the file is too large to be a lab".into()));
    }
    let env: Envelope = serde_json::from_str(s).map_err(|e| ModelError::Invalid(format!("not a NetLabCraft lab: {e}")))?;
    if env.format != FORMAT {
        return Err(ModelError::Invalid(format!("not a NetLabCraft lab (format `{}`)", env.format)));
    }
    if env.version > VERSION {
        return Err(ModelError::Invalid(format!(
            "this lab was saved by a newer NetLabCraft (file version {}, this build reads up to {VERSION})",
            env.version
        )));
    }
    let mut t = env.topology;
    t.sanitize();
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceKind, End};

    #[test]
    fn round_trip() {
        let mut t = Topology::new();
        let a = t.add_device(DeviceKind::Router, None, 1.0, 2.0).unwrap();
        let b = t.add_device(DeviceKind::Switch, None, 3.0, 4.0).unwrap();
        t.connect(End { device: a, port: 0 }, End { device: b, port: 25 }).unwrap();
        t.device_mut(a).unwrap().ports[0].ip = Some("10.1.1.1/24".parse().unwrap());
        let s = to_json(&t).unwrap();
        assert_eq!(from_json(&s).unwrap(), t);
    }

    #[test]
    fn legacy_drawing_keeps_its_single_colour_and_styles_round_trip() {
        let old = r##"{"format":"netlabcraft","version":3,"topology":{"devices":[],"links":[],"next_id":1,
            "annotations":[{"id":1,"x":0,"y":0,"shape":"rectangle","w":10,"h":10,"color":"#aabbcc"}]}}"##;
        let t = from_json(old).unwrap();
        let a = &t.annotations[0];
        assert_eq!(a.color.as_deref(), Some("#aabbcc"));
        assert!(a.has_outline() && !a.no_fill && a.outline_color.is_none() && a.outline_px() == 1);
        // A legacy drawing is saved without the new keys.
        assert!(!to_json(&t).unwrap().contains("outline"));

        let mut t = Topology::new();
        let id = t.add_drawing(crate::Shape::Ellipse, 0.0, 0.0, 30.0, 20.0, None).unwrap();
        let patch = crate::StylePatch { outline: Some(true), outline_color: Some("#112233".into()), outline_width: Some(5), fill: Some(None) };
        t.annotation_mut(id).unwrap().apply_style(&patch).unwrap();
        let back = from_json(&to_json(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        assert!(back.annotations[0].no_fill);
        assert_eq!(back.annotations[0].outline_px(), 5);
    }

    #[test]
    fn hostile_style_values_are_cleaned_on_load() {
        let bad = r##"{"format":"netlabcraft","version":4,"topology":{"devices":[],"links":[],"next_id":1,
            "annotations":[{"id":1,"x":0,"y":0,"shape":"line","w":10,"h":10,"outline_color":"red","outline_width":200}]}}"##;
        let t = from_json(bad).unwrap();
        assert_eq!(t.annotations[0].outline_color, None);
        assert_eq!(t.annotations[0].outline_px(), crate::MAX_OUTLINE_WIDTH);
    }

    #[test]
    fn refuses_bad_styles_without_changing_anything() {
        let mut t = Topology::new();
        let r = t.add_drawing(crate::Shape::Rectangle, 0.0, 0.0, 30.0, 20.0, Some("#aabbcc")).unwrap();
        let l = t.add_drawing(crate::Shape::Line, 0.0, 0.0, 30.0, 20.0, None).unwrap();
        let n = t.add_annotation(0.0, 0.0, "hi").unwrap();
        let before = t.clone();
        let with = |f: fn(&mut crate::StylePatch)| {
            let mut p = crate::StylePatch::default();
            f(&mut p);
            p
        };
        assert!(t.annotation_mut(r).unwrap().apply_style(&with(|p| p.outline_width = Some(0))).is_err());
        assert!(t.annotation_mut(r).unwrap().apply_style(&with(|p| p.outline_width = Some(9))).is_err());
        assert!(t.annotation_mut(r).unwrap().apply_style(&with(|p| p.outline_color = Some("red".into()))).is_err());
        assert!(t.annotation_mut(r).unwrap().apply_style(&with(|p| p.fill = Some(Some("#12".into())))).is_err());
        assert!(t.annotation_mut(l).unwrap().apply_style(&with(|p| p.fill = Some(None))).is_err());
        assert!(t.annotation_mut(l).unwrap().apply_style(&with(|p| p.outline = Some(false))).is_err());
        assert!(t.annotation_mut(n).unwrap().apply_style(&with(|p| p.outline = Some(true))).is_err());
        assert_eq!(t, before);
    }

    #[test]
    fn refuses_foreign_and_future_files() {
        assert!(from_json("{}").is_err());
        assert!(from_json("not json").is_err());
        assert!(from_json(r#"{"format":"other","version":1,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_err());
        assert!(from_json(r#"{"format":"netlabcraft","version":99,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_err());
        assert!(from_json(r#"{"format":"netlabcraft","version":1,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":2,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":3,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":4,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":5,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":6,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":7,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":8,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":9,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":10,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":11,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":12,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":13,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_ok());
        assert!(from_json(r#"{"format":"netlabcraft","version":14,"topology":{"devices":[],"links":[],"next_id":1}}"#).is_err());
    }

    #[test]
    fn netflow_settings_round_trip_and_older_files_open_without_them() {
        use crate::netflow::Destination;
        let mut t = Topology::new();
        let r = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let pc = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        let plain = to_json(&t).unwrap();
        assert!(!plain.contains("netflow"));
        let d = t.device_mut(r).unwrap();
        d.netflow.version = 9;
        d.netflow.destinations.push(Destination { addr: "10.0.0.9".parse().unwrap(), port: 2055 });
        d.netflow.ingress.push("GigabitEthernet0/0".into());
        t.device_mut(pc).unwrap().netflow.collector_port = Some(2055);
        let back = from_json(&to_json(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        assert_eq!(back.device(pc).unwrap().netflow.collector_port, Some(2055));
        let mut v: serde_json::Value = serde_json::from_str(&plain).unwrap();
        v["version"] = 7.into();
        assert!(from_json(&v.to_string()).is_ok());
    }

    #[test]
    fn the_vpn_client_and_a_remote_access_server_round_trip_and_older_files_open_without_them() {
        use crate::vpn::{ClientGroup, LocalPool};
        let mut t = Topology::new();
        let pc = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        let r = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        // Nothing set: no `vpn_client` and no `ra` key at all, so files stay as before.
        let plain = to_json(&t).unwrap();
        assert!(!plain.contains("vpn_client") && !plain.contains("\"ra\""));
        t.device_mut(pc).unwrap().vpn_client.group = "team".into();
        t.device_mut(pc).unwrap().vpn_client.host = "203.0.113.1".into();
        let d = t.device_mut(r).unwrap();
        d.crypto.ra.pools.push(LocalPool { name: "P".into(), start: "10.9.9.1".parse().unwrap(), end: "10.9.9.9".parse().unwrap() });
        d.crypto.ra.groups.push(ClientGroup { name: "team".into(), key: "key-test".into(), pool: Some("P".into()) });
        let back = from_json(&to_json(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        assert_eq!(back.device(pc).unwrap().vpn_client.group, "team");
        // A file saved before version 7 has neither.
        let mut v: serde_json::Value = serde_json::from_str(&plain).unwrap();
        v["version"] = 6.into();
        assert!(from_json(&v.to_string()).is_ok());
    }

    #[test]
    fn a_router_pppoe_setup_round_trips_and_a_default_one_is_left_out() {
        use crate::pppoe::{BbaGroup, DialerSession, PppoeSession};
        let mut t = Topology::new();
        let r = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        assert!(!to_json(&t).unwrap().contains("pppoe"));
        let d = t.device_mut(r).unwrap();
        d.pppoe_router.groups.push(BbaGroup { name: "G".into(), template: Some(1) });
        d.pppoe_router.template_mut(1).unwrap().pool = Some("P".into());
        d.pppoe_router.enabled.insert("GigabitEthernet0/0".into(), "G".into());
        d.pppoe_router.dial_ports.insert("GigabitEthernet0/1".into(), 1);
        d.pppoe_router.dialer_mut(1).unwrap().session = Some(DialerSession {
            port: "GigabitEthernet0/1".into(),
            session: PppoeSession {
                server: r,
                address: "10.0.0.5".parse().unwrap(),
                gateway: "10.0.0.1".parse().unwrap(),
                auth: crate::PppAuth::Chap,
                id: 2,
                server_mac: netlabcraft_core::MacAddr::ZERO,
            },
        });
        let back = from_json(&to_json(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        assert!(back.device(r).unwrap().pppoe_router.active());
    }

    #[test]
    fn a_pppoe_account_and_service_round_trip_and_older_files_open_without_them() {
        use crate::pppoe::{PppoeSession, PppoeUser};
        let mut t = Topology::new();
        let pc = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        let srv = t.add_device(DeviceKind::Server, None, 0.0, 0.0).unwrap();
        // Nothing set: no `pppoe` key at all, so files stay as before.
        assert!(!to_json(&t).unwrap().contains("pppoe"));
        let d = t.device_mut(srv).unwrap();
        d.services.pppoe.on = true;
        d.services.pppoe.pool_size = 5;
        d.services.pppoe.users.push(PppoeUser { name: "ann".into(), password: "pw-test".into() });
        let d = t.device_mut(pc).unwrap();
        d.pppoe.username = "ann".into();
        d.pppoe.session = Some(PppoeSession {
            server: srv,
            address: "10.0.0.5".parse().unwrap(),
            gateway: "10.0.0.1".parse().unwrap(),
            auth: crate::PppAuth::Pap,
            id: 1,
            server_mac: netlabcraft_core::MacAddr::ZERO,
        });
        let back = from_json(&to_json(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        assert!(back.device(pc).unwrap().pppoe.session.is_some());
        // A file saved before version 6 has neither.
        let mut v: serde_json::Value = serde_json::from_str(&to_json(&Topology::new()).unwrap()).unwrap();
        v["version"] = 5.into();
        assert!(from_json(&v.to_string()).is_ok());
    }

    #[test]
    fn a_host_firewall_and_host_files_round_trip_and_older_files_open_with_it_off() {
        use crate::hostfw::{FwProto, HostFwRule};
        let mut t = Topology::new();
        let pc = t.add_device(DeviceKind::Pc, None, 0.0, 0.0).unwrap();
        // A lab saved before the firewall: no `host_fw` key.
        assert!(!to_json(&t).unwrap().contains("host_fw"));
        let mut v: serde_json::Value = serde_json::from_str(&to_json(&t).unwrap()).unwrap();
        v["version"] = 4.into();
        assert!(from_json(&v.to_string()).unwrap().device(pc).unwrap().host_fw.is_default());
        let d = t.device_mut(pc).unwrap();
        d.host_fw.v4.on = true;
        d.host_fw.v4.rules.push(HostFwRule { allow: true, proto: FwProto::Tcp, remote: None, ports: Some((80, 80)) });
        d.files.push(crate::services::WebFile { name: "notes.txt".into(), content: "hello".into() });
        let back = from_json(&to_json(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        assert!(back.device(pc).unwrap().host_fw.v4.on);
    }

    #[test]
    fn a_bgp_process_round_trips_and_an_older_router_has_none() {
        use crate::bgp::{Bgp, Source};
        let mut t = Topology::new();
        let r = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        // A lab saved before BGP: no `bgp` key, and the router opens with no process.
        assert!(!to_json(&t).unwrap().contains("bgp"));
        let mut b = Bgp::new(4_200_000_001);
        b.log_neighbor_changes = true;
        b.networks.push("10.1.0.0/16".parse().unwrap());
        b.redistribute.push(Source::Connected);
        let n = b.neighbor_mut("10.0.0.2".parse().unwrap()).unwrap();
        n.remote_as = Some(65_002);
        n.update_source = Some("Loopback0".into());
        n.ebgp_multihop = Some(2);
        t.device_mut(r).unwrap().bgp = Some(b);
        let back = from_json(&to_json(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        assert!(back.device(r).unwrap().bgp.as_ref().is_some_and(|b| b.asn == 4_200_000_001));
        // A hostile file is cleaned on load: no AS 0, no duplicate neighbours.
        let mut v: serde_json::Value = serde_json::from_str(&to_json(&t).unwrap()).unwrap();
        v["topology"]["devices"][0]["bgp"]["asn"] = 0.into();
        let dup = v["topology"]["devices"][0]["bgp"]["neighbors"][0].clone();
        v["topology"]["devices"][0]["bgp"]["neighbors"].as_array_mut().unwrap().push(dup);
        let clean = from_json(&v.to_string()).unwrap();
        let b = clean.device(r).unwrap().bgp.as_ref().unwrap();
        assert_eq!((b.asn, b.neighbors.len()), (1, 1));
    }

    #[test]
    fn monitor_sessions_round_trip_and_a_switch_without_any_leaves_the_key_out() {
        use crate::span::{SpanDir, SpanSession};
        let mut t = Topology::new();
        let s = t.add_device(DeviceKind::Switch, None, 0.0, 0.0).unwrap();
        let json = to_json(&t).unwrap();
        assert!(!json.contains("\"span\""), "a switch with no session leaves the key out");
        let mut one = SpanSession::new(1);
        one.ports.insert("FastEthernet0/1".into(), SpanDir::Rx);
        one.destination = Some("FastEthernet0/24".into());
        let mut two = SpanSession::new(2);
        two.vlans.insert(netlabcraft_core::VlanId::new(10).unwrap(), SpanDir::Tx);
        t.device_mut(s).unwrap().l2.span = vec![one, two];
        let back = from_json(&to_json(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        // A hostile file is cleaned: a third session and a session number past 66 are dropped.
        let mut v: serde_json::Value = serde_json::from_str(&to_json(&t).unwrap()).unwrap();
        let list = v["topology"]["devices"][0]["l2"]["span"].as_array_mut().unwrap();
        list.push(serde_json::json!({"id": 3, "destination": "FastEthernet0/3"}));
        list.push(serde_json::json!({"id": 200, "destination": "FastEthernet0/4"}));
        let clean = from_json(&v.to_string()).unwrap();
        assert_eq!(clean.device(s).unwrap().l2.span.len(), 2);
    }

    #[test]
    fn prefix_lists_and_route_maps_round_trip_and_an_older_router_has_none() {
        use crate::policy::{PrefixEntry, PrefixList, RouteMap};
        let mut t = Topology::new();
        let r = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let json = to_json(&t).unwrap();
        assert!(!json.contains("prefix_lists") && !json.contains("route_maps"), "a router with no policy leaves the keys out");
        let mut l = PrefixList::new("L");
        l.add(PrefixEntry { seq: 5, permit: true, net: "10.0.0.0/8".parse().unwrap(), ge: None, le: Some(24) }).unwrap();
        let mut m = RouteMap::new("M");
        m.clause_mut(10, true).unwrap().sets.local_pref = Some(200);
        {
            let d = t.device_mut(r).unwrap();
            d.prefix_lists.push(l);
            d.route_maps.push(m);
            let mut b = crate::bgp::Bgp::new(65_001);
            let n = b.neighbor_mut("10.0.0.2".parse().unwrap()).unwrap();
            n.remote_as = Some(65_002);
            n.route_map_in = Some("M".into());
            n.prefix_list_out = Some("L".into());
            b.redistribute.push(crate::bgp::Source::Connected);
            b.redistribute_maps.insert("connected".into(), "M".into());
            d.bgp = Some(b);
            let mut st = crate::redist::Redist::new(crate::redist::RedistSource::Static);
            st.route_map = Some("M".into());
            d.ospf = Some(crate::Ospf { process: 1, redistribute: vec![st], ..crate::Ospf::default() });
        }
        let back = from_json(&to_json(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        // A hostile file is cleaned: a list with an impossible range loses that line.
        let mut v: serde_json::Value = serde_json::from_str(&to_json(&t).unwrap()).unwrap();
        v["topology"]["devices"][0]["prefix_lists"][0]["entries"][0]["ge"] = 2.into();
        let clean = from_json(&v.to_string()).unwrap();
        assert!(clean.device(r).unwrap().prefix_lists.first().is_some_and(|l| l.entries.is_empty()));
    }

    #[test]
    fn a_version_1_phone_opens_with_a_cellular_port_and_cell_settings_round_trip() {
        let mut t = Topology::new();
        let ph = t.add_model(crate::models::find("Smartphone").unwrap(), None, 0.0, 0.0).unwrap();
        // As saved before the cellular radio: version 1, one port.
        let mut v: serde_json::Value = serde_json::from_str(&to_json(&t).unwrap()).unwrap();
        v["version"] = 1.into();
        v["topology"]["devices"][0]["ports"].as_array_mut().unwrap().truncate(1);
        let old = v.to_string();
        assert!(!old.contains("Cellular0"), "{old}");
        let back = from_json(&old).unwrap();
        assert_eq!(back.device(ph).unwrap().ports.len(), 2);
        assert_eq!(back.device(ph).unwrap().ports[1].name, "Cellular0");
        let mut t2 = back;
        t2.device_mut(ph).unwrap().cell.apn = "internet".into();
        t2.device_mut(ph).unwrap().cell.data_off = true;
        let again = from_json(&to_json(&t2).unwrap()).unwrap();
        assert_eq!(again.device(ph).unwrap().cell, t2.device(ph).unwrap().cell);
    }
}
