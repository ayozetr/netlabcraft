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
    "activity-first-router",
    "home-internet",
    "modular",
    "switch-card",
    "ip-phones",
    "sip-phones",
    "smart-home",
    "iot",
    "wlc",
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
    // `-M` prints plain numbers, `-E` the link type by its short name (`ether`, `ieee-802-11`), `-c` the packet count.
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

/// Reads one capture with the tools and returns what is wrong with it, if anything.
fn problems(label: &str, path: &Path, frames: u64, encapsulation: &str) -> Vec<String> {
    let mut out = Vec::new();
    let info = capinfos(path);
    if info.encapsulation != encapsulation {
        out.push(format!("{label}: link type {} instead of {encapsulation}", info.encapsulation));
    }
    if info.packets != frames {
        out.push(format!("{label}: capinfos counts {} packets, the export reported {frames}", info.packets));
    }
    let bad = malformed(path);
    if !bad.is_empty() {
        out.push(format!("{label}: tshark calls {} frame(s) malformed (number, protocol): {}", bad.len(), bad.join("; ")));
    }
    out
}

/// How many frames of the capture match a Wireshark display filter.
fn matching(path: &Path, filter: &str) -> usize {
    let file = path.to_str().unwrap();
    let out = process::run("tshark", &["-r", file, "-n", "-Y", filter, "-T", "fields", "-e", "frame.number"], TIMEOUT)
        .unwrap_or_else(|e| panic!("{file}: {e}"));
    out.stdout.lines().filter(|l| !l.trim().is_empty()).count()
}

#[test]
fn wired_captures_are_valid_pcap_without_malformed_frames() {
    if !tools_present() {
        return;
    }
    let mut empty = Vec::new();
    let mut failures = Vec::new();
    for sample in SAMPLES {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": sample})).unwrap();
        s.execute("sim.advance", json!({"seconds": 40})).unwrap();
        let (path, frames) = export(&mut s, &format!("{sample}.pcap"), "ethernet");
        // A lab with no routing protocol or spanning tree sends nothing by itself, so its capture is empty and has nothing to check.
        if frames == 0 {
            empty.push(*sample);
            continue;
        }
        failures.extend(problems(sample, &path, frames, "ether"));
        if *sample == "wlc" {
            // DIAG
            let f = path.to_str().unwrap();
            let v = process::run("tshark", &["-r", f, "-n", "-V", "-Y", "_ws.malformed", "-c", "2"], TIMEOUT).unwrap();
            eprintln!("DIAG wlc malformed:\n{}", v.stdout);
            let x = process::run("tshark", &["-r", f, "-n", "-x", "-Y", "_ws.malformed", "-c", "1"], TIMEOUT).unwrap();
            eprintln!("DIAG wlc hex:\n{}", x.stdout);
        }
    }
    eprintln!("{} labs checked, {} with an empty capture: {empty:?}", SAMPLES.len() - empty.len(), empty.len());
    // Every lab is read before the test fails, so one run shows all the damage.
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // Most labs must have been checked, or the test is not looking at anything.
    assert!(SAMPLES.len() - empty.len() >= 10, "too many labs with an empty capture: {empty:?}");
}

/// A lab, what a user types in it, and the protocols the capture must then hold (Wireshark display filters).
struct Traffic {
    sample: &'static str,
    typed: &'static [(&'static str, &'static str)],
    expect: &'static [&'static str],
}

const TRAFFIC: &[Traffic] = &[
    Traffic { sample: "vlans", typed: &[("PC1", "ping 192.168.20.10")], expect: &["arp", "icmp.type == 8", "icmp.type == 0"] },
    Traffic {
        sample: "two-subnets",
        typed: &[("PC1", "ping 10.0.2.10"), ("PC1", "tracert 10.0.2.10")],
        expect: &["arp", "icmp.type == 8", "icmp.type == 0"],
    },
    Traffic {
        sample: "nat-acl",
        // PC1 gets out through NAT; PC2 is refused by the access list.
        typed: &[("PC1", "ping 93.184.216.34"), ("PC2", "ping 93.184.216.34")],
        expect: &["icmp.type == 8", "icmp.type == 0"],
    },
    Traffic {
        sample: "services",
        typed: &[("PC1", "nslookup www.netlabcraft.lab"), ("PC1", "ping www.netlabcraft.lab")],
        expect: &["dhcp", "dns", "icmp.type == 8"],
    },
    Traffic { sample: "dhcp", typed: &[("PC1", "ping 192.168.50.1")], expect: &["dhcp", "icmp.type == 8", "icmp.type == 0"] },
];

#[test]
fn traffic_typed_by_a_user_is_valid_pcap() {
    if !tools_present() {
        return;
    }
    let mut failures = Vec::new();
    for t in TRAFFIC {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": t.sample})).unwrap();
        // Let DHCP and the first protocol exchanges settle before the user types.
        s.execute("sim.advance", json!({"seconds": 10})).unwrap();
        for (device, line) in t.typed {
            s.execute("cli.exec", json!({"device": device, "line": line})).unwrap();
        }
        s.execute("sim.advance", json!({"seconds": 5})).unwrap();
        let (path, frames) = export(&mut s, &format!("{}-traffic.pcap", t.sample), "ethernet");
        failures.extend(problems(t.sample, &path, frames, "ether"));
        // DIAG
        let f = path.to_str().unwrap();
        let phs = process::run("tshark", &["-r", f, "-n", "-q", "-z", "io,phs"], TIMEOUT).unwrap();
        eprintln!("DIAG {} phs:\n{}", t.sample, phs.stdout);
        let ports = process::run("tshark", &["-r", f, "-n", "-Y", "udp.port == 67 || udp.port == 68 || bootp", "-T", "fields", "-e", "frame.number", "-e", "udp.srcport", "-e", "udp.dstport", "-e", "_ws.col.info"], TIMEOUT).unwrap();
        eprintln!("DIAG {} dhcp-ish:\n{}", t.sample, ports.stdout);
        for filter in t.expect {
            if matching(&path, filter) == 0 {
                failures.push(format!("{}: no frame matches `{filter}` in {frames} frames", t.sample));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_ping_gets_its_reply_before_the_capture_is_read() {
    if !tools_present() {
        return;
    }
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "vlans"})).unwrap();
    let r = s.execute("cli.exec", json!({"device": "PC1", "line": "ping 192.168.20.10"})).unwrap();
    let text = r.get("output").and_then(Value::as_str).unwrap_or("");
    assert!(text.contains("reply from 192.168.20.10"), "the ping did not get through:\n{text}");
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
    let failures = problems("wifi", &path, frames, "ieee-802-11");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
