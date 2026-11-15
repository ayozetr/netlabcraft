//! Engine tests for the Traffic Generator: a bounded burst of ICMP, UDP or TCP packets, counted.

use serde_json::{Value, json};

use crate::Session;

fn lab() -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "lan"})).unwrap();
    s
}

fn send(s: &mut Session, p: Value) -> crate::Result<Value> {
    let mut p = p;
    p["device"] = json!("PC1");
    s.execute("traffic.send", p)
}

#[test]
fn a_burst_of_pings_is_counted_and_listed_in_order() {
    let mut s = lab();
    let st = s.execute("traffic.status", json!({"device": "PC1"})).unwrap();
    assert_eq!((st["ran"].clone(), st["max_count"].clone()), (json!(false), json!(100)));
    let r = send(&mut s, json!({"to": "192.168.1.12", "count": 5, "interval": 0.5, "size": 64})).unwrap();
    assert_eq!((r["sent"].clone(), r["protocol"].clone(), r["size"].clone()), (json!(5), json!("ICMP"), json!(64)));
    // The first echo waits for ARP and may be lost; the rest are answered.
    assert!(r["received"].as_u64().unwrap_or(0) >= 4, "{r}");
    assert_eq!(r["received"].as_u64().unwrap_or(0) + r["lost"].as_u64().unwrap_or(0), 5);
    let list = r["results"].as_array().unwrap();
    assert_eq!(list.iter().filter_map(|x| x["n"].as_u64()).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
    let times: Vec<f64> = list.iter().filter_map(|x| x["at"].as_f64()).collect();
    assert!(times.windows(2).all(|w| w[1] - w[0] >= 0.499), "packets are an interval apart: {times:?}");
    // The status keeps the last burst, and the packets are in the event list.
    assert_eq!(s.execute("traffic.status", json!({"device": "PC1"})).unwrap()["sent"], json!(5));
    assert_eq!(s.execute("traffic.status", json!({"device": "PC2"})).unwrap()["ran"], json!(false));
    assert!(s.execute("sim.events", json!({})).unwrap().to_string().contains("ICMP"));
    // A device name works as the destination,.
    let again = send(&mut s, json!({"to": "PC3", "count": 2, "interval": 0})).unwrap();
    assert_eq!(again["sent"], json!(2));
}

#[test]
fn what_is_blocked_or_closed_is_lost() {
    let mut s = lab();
    s.execute("host.firewall", json!({"device": "PC2", "on": true})).unwrap();
    let r = send(&mut s, json!({"to": "192.168.1.12", "count": 3, "interval": 0.1})).unwrap();
    assert_eq!((r["sent"].clone(), r["received"].clone(), r["lost"].clone()), (json!(3), json!(0), json!(3)));
    s.execute("host.firewall", json!({"device": "PC2", "on": false})).unwrap();
    // Nothing listens on TCP 8080 of a PC.
    let r = send(&mut s, json!({"to": "192.168.1.12", "protocol": "tcp", "port": 8080, "count": 2, "interval": 0})).unwrap();
    assert_eq!((r["protocol"].clone(), r["port"].clone(), r["received"].clone()), (json!("TCP"), json!(8080), json!(0)));
    let r = send(&mut s, json!({"to": "192.168.1.12", "protocol": "UDP", "port": 9, "count": 2, "interval": 0})).unwrap();
    assert_eq!((r["protocol"].clone(), r["sent"].clone()), (json!("UDP"), json!(2)));
    // No route: every packet is lost with a reason, and the command still answers.
    let r = send(&mut s, json!({"to": "172.16.0.1", "count": 2, "interval": 0})).unwrap();
    assert_eq!(r["lost"], json!(2));
    assert!(r["results"][0]["note"].as_str().is_some_and(|n| !n.is_empty()));
}

#[test]
fn the_burst_is_bounded_and_the_parameters_are_checked() {
    let mut s = lab();
    for p in [
        json!({}),
        json!({"to": "192.168.1.12", "count": 0}),
        json!({"to": "192.168.1.12", "count": 101}),
        json!({"to": "192.168.1.12", "interval": 5.5}),
        json!({"to": "192.168.1.12", "interval": -1}),
        json!({"to": "192.168.1.12", "size": 1401}),
        json!({"to": "192.168.1.12", "protocol": "gre"}),
        json!({"to": "192.168.1.12", "protocol": "udp"}),
        json!({"to": "192.168.1.12", "protocol": "tcp", "port": 0}),
        json!({"to": "192.168.1.12", "protocol": "tcp", "port": 70000}),
        json!({"to": "no such device"}),
    ] {
        assert!(send(&mut s, p.clone()).is_err(), "{p}");
    }
    // The largest burst runs to its end.
    let r = send(&mut s, json!({"to": "192.168.1.12", "count": 100, "interval": 0, "size": 0})).unwrap();
    assert_eq!(r["sent"], json!(100));
    // A switch and a printer have no Traffic Generator.
    assert!(s.execute("traffic.send", json!({"device": "S1", "to": "192.168.1.12"})).is_err());
    assert!(s.execute("traffic.status", json!({"device": "Printer1"})).is_err());
}
