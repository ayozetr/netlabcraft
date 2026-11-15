//! `show interfaces`: MTU, bandwidth, delay, load and the packet counts the port really kept
//! (console depth round of 2026-11-15).

use super::Lab;

/// `(packets, bytes)` of the "packets input" or "packets output" line.
fn counts(out: &str, dir: &str) -> (u64, u64) {
    let line = out.lines().find(|l| l.contains(&format!("packets {dir},"))).unwrap_or("");
    let w: Vec<&str> = line.split_whitespace().collect();
    (w.first().and_then(|x| x.parse().ok()).unwrap_or(0), w.get(3).and_then(|x| x.parse().ok()).unwrap_or(0))
}

#[test]
fn an_interface_shows_its_size_speed_delay_and_the_traffic_it_carried() {
    let mut lab = Lab::new();
    let r = lab.add(netlabcraft_model::DeviceKind::Router);
    let a = lab.add(netlabcraft_model::DeviceKind::Pc);
    lab.cable(a, "fa0", r, "g0/0");
    lab.run(r, "en\nconf t\ninterface g0/0\nip address 192.168.1.1 255.255.255.0\nno shutdown\nend");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    let before = lab.run(r, "show interfaces g0/0");
    assert!(before.contains("MTU 1500 bytes, BW 1000000 Kbit/sec, DLY 10 usec"), "{before}");
    assert!(before.contains("Reliability 255/255, txload 1/255, rxload 1/255"), "{before}");
    assert!(before.contains("Encapsulation ARPA"), "{before}");
    assert!(before.contains("Counters: never cleared"), "{before}");
    let (in0, _) = counts(&before, "input");
    let (out0, _) = counts(&before, "output");
    assert!(lab.run(a, "ping 192.168.1.1").contains("4 answered"));
    let after = lab.run(r, "show interfaces g0/0");
    let (in1, in_bytes) = counts(&after, "input");
    let (out1, out_bytes) = counts(&after, "output");
    // Four echoes came in and four replies went out, at least (and an ARP exchange on top).
    assert!(in1 >= in0 + 4 && out1 >= out0 + 4, "{before}\n{after}");
    assert!(in_bytes > 0 && out_bytes > 0, "{after}");
    // The one-port form and the whole list say the same.
    let all = lab.run(r, "show interfaces");
    assert!(all.contains(&format!("{in1} packets input, {in_bytes} bytes")), "{all}");
}

#[test]
fn bandwidth_delay_and_ip_mtu_settings_show_and_clear_counters_zeroes_the_counts() {
    let mut lab = Lab::new();
    let r = lab.add(netlabcraft_model::DeviceKind::Router);
    let a = lab.add(netlabcraft_model::DeviceKind::Pc);
    lab.cable(a, "fa0", r, "g0/0");
    lab.run(r, "en\nconf t\ninterface g0/0\nip address 192.168.1.1 255.255.255.0\nno shutdown\nbandwidth 1\ndelay 50\nip mtu 1400\nend");
    lab.run(a, "ipconfig 192.168.1.10 255.255.255.0 192.168.1.1");
    lab.run(a, "ping 192.168.1.1");
    let busy = lab.run(r, "show interfaces g0/0");
    assert!(busy.contains("MTU 1500 bytes (IP MTU 1400), BW 1 Kbit/sec, DLY 500 usec"), "{busy}");
    // A link of 1 kbit/s that carried frames is not idle.
    assert!(!busy.contains("txload 1/255"), "{busy}");
    assert!(counts(&busy, "input").0 > 0);
    lab.run(r, "clear counters");
    let cleared = lab.run(r, "show interfaces g0/0");
    assert!(cleared.contains("Counters: cleared 0:00:"), "{cleared}");
    assert!(cleared.contains("0 packets input, 0 bytes, 0 broadcasts, 0 multicasts"), "{cleared}");
    assert!(cleared.contains("0 packets output, 0 bytes, 0 broadcasts, 0 multicasts"), "{cleared}");
    // New traffic counts from zero.
    lab.run(a, "ping 192.168.1.1");
    assert!(counts(&lab.run(r, "show interfaces g0/0"), "input").0 >= 4);
}
