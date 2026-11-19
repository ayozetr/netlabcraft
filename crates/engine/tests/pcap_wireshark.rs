//! Reads the captures NetLabCraft writes with Wireshark's own command-line tools, so "valid pcap" is checked by a
//! program we did not write.
//!
//! For each capture it asks `capinfos` for the link type and the packet count (they must match what the export
//! reported), has `tshark` read the whole file (a damaged file ends with an error) and lists the frames `tshark`
//! calls malformed (there must be none).
//!
//! The tools run as separate programs; nothing of Wireshark is linked, copied or read. Without them the test says so
//! and passes, unless `NETLABCRAFT_REQUIRE_WIRESHARK` is set (CI sets it), which turns a missing tool into a failure.

use std::path::{Path, PathBuf};
use std::time::Duration;

use netlabcraft_core::process;
use netlabcraft_engine::Session;
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(60);

/// Labs whose event lists hold a good spread of protocols after a little simulated time.
const SAMPLES: &[&str] = &[
    "lan",
    "two-subnets",
    "vlans",
    "static-routing",
    "stp",
    "etherchannel",
    "vtp",
    "ipv6",
    "multilayer",
    "rip",
    "serial-wan",
    "hsrp",
    "ospf",
    "nat-acl",
    "services",
    "dhcp",
    "firewall",
];

fn tools_present() -> bool {
    let present = process::run("tshark", &["--version"], TIMEOUT).is_ok() && process::run("capinfos", &["--version"], TIMEOUT).is_ok();
    if !present {
        assert!(
            std::env::var_os("NETLABCRAFT_REQUIRE_WIRESHARK").is_none(),
            "tshark and capinfos are required here (NETLABCRAFT_REQUIRE_WIRESHARK is set) and were not found"
        );
        eprintln!("skipped: tshark and capinfos are not installed (set NETLABCRAFT_REQUIRE_WIRESHARK=1 to make that a failure)");
    }
    present
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("netlabcraft-pcap-wireshark-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// What `capinfos` reports about a capture file.
struct Info {
    encapsulation: String,
    packets: u64,
}

fn capinfos(path: &Path) -> Info {
    let file = path.to_str().unwrap();
    // `-M` prints plain numbers, `-E` the link type, `-c` the packet count.
    let out = process::run("capinfos", &["-M", "-E", "-c", file], TIMEOUT).unwrap_or_else(|e| panic!("{file}: {e}"));
    let field = |name: &str| -> String {
        out.stdout
            .lines()
            .find_map(|l| l.strip_prefix(name).and_then(|v| v.strip_prefix(':')))
            .unwrap_or_else(|| panic!("{file}: no `{name}` in:\n{}", out.stdout))
            .trim()
            .to_string()
    };
    Info {
        encapsulation: field("File encapsulation"),
        packets: field("Number of packets").replace([',', ' '], "").parse().unwrap_or_else(|e| panic!("{file}: packet count: {e}")),
    }
}

/// Has `tshark` read every packet, and returns the numbers of the frames it calls malformed.
fn malformed(path: &Path) -> Vec<String> {
    let file = path.to_str().unwrap();
    // Reading the whole file with no output: a damaged capture is a non-zero exit.
    process::run("tshark", &["-r", file, "-n", "-q"], TIMEOUT).unwrap_or_else(|e| panic!("{file}: {e}"));
    let out =
        process::run("tshark", &["-r", file, "-n", "-Y", "_ws.malformed", "-T", "fields", "-e", "frame.number", "-e", "_ws.col.protocol"], TIMEOUT)
            .unwrap_or_else(|e| panic!("{file}: {e}"));
    out.stdout.lines().map(|l| l.replace('\t', " ")).collect()
}

fn export(s: &mut Session, name: &str, link: &str) -> (PathBuf, u64) {
    let path = scratch(name);
    let r = s.execute("file.export_pcap", json!({"link": link, "path": path.to_str().unwrap()})).unwrap();
    (path, r["frames"].as_u64().unwrap())
}

#[test]
fn wired_captures_are_valid_pcap_without_malformed_frames() {
    if !tools_present() {
        return;
    }
    for sample in SAMPLES {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": sample})).unwrap();
        s.execute("sim.advance", json!({"seconds": 40})).unwrap();
        let (path, frames) = export(&mut s, &format!("{sample}.pcap"), "ethernet");
        assert!(frames > 0, "{sample}: the capture is empty");
        let info = capinfos(&path);
        assert_eq!(info.encapsulation, "Ethernet", "{sample}");
        assert_eq!(info.packets, frames, "{sample}: capinfos counts a different number of packets than the export reported");
        let bad = malformed(&path);
        assert!(bad.is_empty(), "{sample}: tshark calls {} frame(s) malformed (number, protocol):\n{}", bad.len(), bad.join("\n"));
    }
}

#[test]
fn a_ping_through_the_vlan_lab_is_valid_pcap() {
    if !tools_present() {
        return;
    }
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "vlans"})).unwrap();
    let r = s.execute("cli.exec", json!({"device": "PC1", "line": "ping 192.168.20.10"})).unwrap();
    let text = r.get("output").and_then(Value::as_str).unwrap_or("");
    assert!(text.contains("Reply from 192.168.20.10"), "the ping did not get through:\n{text}");
    let (path, frames) = export(&mut s, "vlans-ping.pcap", "ethernet");
    assert_eq!(capinfos(&path).packets, frames);
    let bad = malformed(&path);
    assert!(bad.is_empty(), "tshark calls {} frame(s) malformed (number, protocol):\n{}", bad.len(), bad.join("\n"));
    // The echo request and its reply are there for an analyser to find.
    let icmp = process::run("tshark", &["-r", path.to_str().unwrap(), "-n", "-Y", "icmp", "-T", "fields", "-e", "icmp.type"], TIMEOUT).unwrap();
    assert!(icmp.stdout.lines().any(|l| l.trim() == "8"), "no echo request in the capture");
    assert!(icmp.stdout.lines().any(|l| l.trim() == "0"), "no echo reply in the capture");
}

#[test]
fn the_wireless_capture_is_valid_80211_pcap() {
    if !tools_present() {
        return;
    }
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "wifi"})).unwrap();
    s.execute("sim.advance", json!({"seconds": 40})).unwrap();
    let (path, frames) = export(&mut s, "wifi.pcap", "802.11");
    assert!(frames > 0, "the air capture is empty");
    let info = capinfos(&path);
    assert!(info.encapsulation.contains("802.11"), "link type: {}", info.encapsulation);
    assert_eq!(info.packets, frames);
    let bad = malformed(&path);
    assert!(bad.is_empty(), "tshark calls {} frame(s) malformed (number, protocol):\n{}", bad.len(), bad.join("\n"));
}
