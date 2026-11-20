//! Reads the captures NetLabCraft writes with Wireshark's own command-line tools, so "valid pcap" is checked by a
//! program we did not write.
//!
//! For each capture it asks `capinfos` for the file type, the link type and the packet count (they must match what the
//! export reported), has `tshark` read the whole file (a damaged file ends with an error), lists the frames `tshark`
//! calls malformed and the frames whose IP, TCP, UDP, ICMP or ICMPv6 checksum is wrong (there must be none of either).
//! It also prints which protocols `tshark` recognised, and notes (without failing) what it would still expect:
//! a note that keeps showing up is a candidate to become a hard check.
//!
//! Besides the labs as they open, it types pings, `tracert`, NAT and ACL traffic, DNS lookups, DHCP leases and IPv6
//! into some of them, fires one PDU per application (`pdu.complex`) and reads a wireless lab's air capture.
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
    file_type: String,
    encapsulation: String,
    packets: u64,
}

fn capinfos(path: &Path) -> Info {
    let file = path.to_str().unwrap();
    // `-M` prints plain numbers, `-t` the file type, `-E` the link type by its short name (`ether`, `ieee-802-11`), `-c` the packet count.
    let out = process::run("capinfos", &["-M", "-t", "-E", "-c", file], TIMEOUT).unwrap_or_else(|e| panic!("{file}: {e}"));
    let field = |name: &str| -> String {
        out.stdout
            .lines()
            .find_map(|l| l.strip_prefix(name).and_then(|v| v.strip_prefix(':')))
            .unwrap_or_else(|| panic!("{file}: no `{name}` in:\n{}", out.stdout))
            .trim()
            .to_string()
    };
    Info {
        file_type: field("File type"),
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

/// The frame numbers of a capture that match a Wireshark display filter.
fn frames_matching(path: &Path, filter: &str) -> Vec<String> {
    let file = path.to_str().unwrap();
    let out = process::run("tshark", &["-r", file, "-n", "-Y", filter, "-T", "fields", "-e", "frame.number"], TIMEOUT)
        .unwrap_or_else(|e| panic!("{file}: {filter}: {e}"));
    out.stdout.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

/// A frame whose checksum `tshark` finds wrong (status 0), for every protocol that carries one that `tshark` checks.
const BAD_CHECKSUM: &str =
    "ip.checksum.status == 0 || tcp.checksum.status == 0 || udp.checksum.status == 0 || icmp.checksum.status == 0 || icmpv6.checksum.status == 0";

/// What a run found: `failures` fail the test at the end, `notes` are printed and nothing more.
#[derive(Default)]
struct Report {
    failures: Vec<String>,
    notes: Vec<String>,
}

impl Report {
    /// Reads one capture with the tools; whatever is wrong with it becomes a failure.
    fn check_file(&mut self, label: &str, path: &Path, frames: u64, link: &str, file_type_suffix: &str) {
        let info = capinfos(path);
        if !info.file_type.ends_with(file_type_suffix) {
            self.failures.push(format!("{label}: file type `{}`, expected one ending in `{file_type_suffix}`", info.file_type));
        }
        if info.encapsulation != link {
            self.failures.push(format!("{label}: link type {} instead of {link}", info.encapsulation));
        }
        if info.packets != frames {
            self.failures.push(format!("{label}: capinfos counts {} packets, the export reported {frames}", info.packets));
        }
        let bad = malformed(path);
        if !bad.is_empty() {
            self.failures.push(format!("{label}: tshark calls {} frame(s) malformed (number, protocol): {}", bad.len(), bad.join("; ")));
        }
        let wrong = frames_matching(path, BAD_CHECKSUM);
        if !wrong.is_empty() {
            self.failures.push(format!("{label}: {} frame(s) with a wrong checksum, frame numbers: {}", wrong.len(), wrong.join(", ")));
        }
        // Softer smells, reported but not failing yet.
        let cut = frames_matching(path, "frame.cap_len < frame.len");
        if !cut.is_empty() {
            self.notes.push(format!("{label}: {} frame(s) captured shorter than their length: {}", cut.len(), cut.join(", ")));
        }
        let back = frames_matching(path, "frame.time_delta < 0");
        if !back.is_empty() {
            self.notes.push(format!("{label}: time goes backwards at {} frame(s): {}", back.len(), back.join(", ")));
        }
        self.notes.push(format!("{label}: {} frames, protocols {}", frames, protocols(path).join(" ")));
    }

    /// Frames that must be in the capture.
    fn require(&mut self, label: &str, path: &Path, filters: &[&str]) {
        for f in filters {
            if frames_matching(path, f).is_empty() {
                self.failures.push(format!("{label}: no frame matches `{f}`"));
            }
        }
    }

    /// Frames that would be good to see; their absence is only noted.
    fn hope(&mut self, label: &str, path: &Path, filters: &[&str]) {
        for f in filters {
            if frames_matching(path, f).is_empty() {
                self.notes.push(format!("{label}: NOTE no frame matches `{f}`"));
            }
        }
    }

    fn finish(self) {
        for n in &self.notes {
            eprintln!("{n}");
        }
        // TEMP-SHOW-NOTES
        if !self.notes.is_empty() {
            panic!("NOTES\n{}\nFAILURES\n{}", self.notes.join("\n"), self.failures.join("\n"));
        }
        assert!(self.failures.is_empty(), "{}", self.failures.join("\n"));
    }
}

/// The protocols `tshark` found in a capture, outermost first, each once.
fn protocols(path: &Path) -> Vec<String> {
    let file = path.to_str().unwrap();
    let out = process::run("tshark", &["-r", file, "-n", "-q", "-z", "io,phs"], TIMEOUT).unwrap_or_else(|e| panic!("{file}: {e}"));
    let mut seen: Vec<String> = Vec::new();
    for line in out.stdout.lines().filter(|l| l.contains("frames:")) {
        if let Some(name) = line.split_whitespace().next()
            && !seen.iter().any(|n| n == name)
        {
            seen.push(name.to_string());
        }
    }
    seen
}

/// Protocols a lab's own traffic should show (all labs have run 40 simulated seconds), noted when `tshark` does not see them.
const PASSIVE: &[(&str, &[&str])] = &[
    ("vlans", &["stp", "vlan"]),
    ("stp", &["stp"]),
    ("etherchannel", &["lacp"]),
    ("vtp", &["vtp"]),
    ("rip", &["rip"]),
    ("ospf", &["ospf"]),
    ("hsrp", &["hsrp"]),
    ("ipv6", &["icmpv6"]),
    ("wlc", &["capwap"]),
    ("sip-phones", &["sip"]),
    ("ip-phones", &["cdp"]),
    ("lan", &["stp"]),
];

#[test]
fn wired_captures_are_valid_pcap_without_malformed_frames() {
    if !tools_present() {
        return;
    }
    let mut empty = Vec::new();
    let mut rep = Report::default();
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
        rep.check_file(sample, &path, frames, "ether", "pcap");
        for (lab, filters) in PASSIVE {
            if lab == sample {
                rep.hope(sample, &path, filters);
            }
        }
    }
    eprintln!("{} labs checked, {} with an empty capture: {empty:?}", SAMPLES.len() - empty.len(), empty.len());
    // Most labs must have been checked, or the test is not looking at anything.
    if SAMPLES.len() - empty.len() < 10 {
        rep.failures.push(format!("too many labs with an empty capture: {empty:?}"));
    }
    // Every lab is read before the test fails, so one run shows all the damage.
    rep.finish();
}

/// A lab, what a user types in it, the frames the capture must then hold and the ones it should hold (Wireshark display filters).
struct Traffic {
    sample: &'static str,
    typed: &'static [(&'static str, &'static str)],
    expect: &'static [&'static str],
    hope: &'static [&'static str],
}

const TRAFFIC: &[Traffic] = &[
    Traffic {
        sample: "vlans",
        typed: &[("PC1", "ping 192.168.20.10")],
        expect: &["arp", "icmp.type == 8", "icmp.type == 0", "vlan"],
        hope: &["stp"],
    },
    Traffic {
        sample: "two-subnets",
        typed: &[("PC1", "ping 10.0.2.10"), ("PC1", "tracert 10.0.2.10")],
        expect: &["arp", "icmp.type == 8", "icmp.type == 0"],
        hope: &["icmp.type == 11"],
    },
    Traffic {
        sample: "nat-acl",
        // PC1 gets out through NAT; PC2 is refused by the access list.
        typed: &[("PC1", "ping 93.184.216.34"), ("PC2", "ping 93.184.216.34")],
        expect: &["icmp.type == 8", "icmp.type == 0"],
        hope: &["icmp.type == 3"],
    },
    Traffic {
        sample: "services",
        typed: &[("PC1", "ipconfig /renew"), ("PC1", "nslookup www.netlabcraft.lab"), ("PC1", "ping www.netlabcraft.lab")],
        expect: &["dhcp", "dns", "icmp.type == 8"],
        hope: &["dns.flags.response == 1", "dhcp.option.dhcp == 5"],
    },
    Traffic {
        sample: "dhcp",
        typed: &[("PC1", "ipconfig /renew"), ("PC1", "ping 192.168.50.1")],
        expect: &["dhcp", "icmp.type == 8", "icmp.type == 0"],
        hope: &["dhcp.option.dhcp == 1", "dhcp.option.dhcp == 2", "dhcp.option.dhcp == 3", "dhcp.option.dhcp == 5"],
    },
    Traffic {
        sample: "ipv6",
        typed: &[("PC1", "ping 2001:db8:2::10")],
        expect: &["icmpv6"],
        hope: &["icmpv6.type == 128", "icmpv6.type == 129", "icmpv6.type == 135", "icmpv6.type == 136", "icmpv6.type == 134"],
    },
];

#[test]
fn traffic_typed_by_a_user_is_valid_pcap() {
    if !tools_present() {
        return;
    }
    let mut rep = Report::default();
    for t in TRAFFIC {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": t.sample})).unwrap();
        // Let DHCP and the first protocol exchanges settle before the user types.
        s.execute("sim.advance", json!({"seconds": 10})).unwrap();
        for (device, line) in t.typed {
            if let Err(e) = s.execute("cli.exec", json!({"device": device, "line": line})) {
                rep.failures.push(format!("{}: `{device}` could not run `{line}`: {e}", t.sample));
            }
        }
        s.execute("sim.advance", json!({"seconds": 5})).unwrap();
        let label = format!("{} (typed)", t.sample);
        let (path, frames) = export(&mut s, &format!("{}-traffic.pcap", t.sample), "ethernet");
        rep.check_file(&label, &path, frames, "ether", "pcap");
        rep.require(&label, &path, t.expect);
        rep.hope(&label, &path, t.hope);
    }
    rep.finish();
}

/// One PDU per application: (application, extra parameters, `to`, frames it must put on the wire, frames it should).
struct App {
    app: &'static str,
    extra: &'static str,
    to: Option<&'static str>,
    expect: &'static [&'static str],
    hope: &'static [&'static str],
}

const APPS: &[App] = &[
    App { app: "dns", extra: r#"{"text": "www.netlabcraft.lab"}"#, to: Some("Server0"), expect: &["udp.dstport == 53"], hope: &["dns"] },
    App { app: "http", extra: "{}", to: Some("Server0"), expect: &["tcp.dstport == 80 && tcp.flags.syn == 1"], hope: &["http"] },
    App { app: "telnet", extra: "{}", to: Some("Server0"), expect: &["tcp.dstport == 23 && tcp.flags.syn == 1"], hope: &["telnet"] },
    App { app: "ssh", extra: "{}", to: Some("Server0"), expect: &["tcp.dstport == 22 && tcp.flags.syn == 1"], hope: &["ssh"] },
    App { app: "ftp", extra: "{}", to: Some("Server0"), expect: &["tcp.dstport == 21 && tcp.flags.syn == 1"], hope: &["ftp"] },
    App { app: "smtp", extra: "{}", to: Some("Server0"), expect: &["tcp.dstport == 25 && tcp.flags.syn == 1"], hope: &["smtp"] },
    App { app: "pop3", extra: "{}", to: Some("Server0"), expect: &["tcp.dstport == 110 && tcp.flags.syn == 1"], hope: &["pop"] },
    App { app: "tftp", extra: r#"{"text": "notes.txt"}"#, to: Some("Server0"), expect: &["udp.dstport == 69"], hope: &["tftp"] },
    App { app: "snmp", extra: "{}", to: Some("Server0"), expect: &["udp.dstport == 161"], hope: &["snmp"] },
    App {
        app: "udp",
        extra: r#"{"src_port": 4000, "dst_port": 5000, "size": 100}"#,
        to: Some("Server0"),
        expect: &["udp.dstport == 5000"],
        hope: &[],
    },
    App {
        app: "tcp",
        extra: r#"{"src_port": 4001, "dst_port": 8080}"#,
        to: Some("Server0"),
        expect: &["tcp.dstport == 8080 && tcp.flags.syn == 1"],
        hope: &[],
    },
    App {
        app: "ping",
        extra: r#"{"ttl": 9, "seq": 7, "size": 64, "dscp": "ef"}"#,
        to: Some("Server0"),
        expect: &["icmp.type == 8 && ip.ttl == 9", "icmp.type == 0", "ip.dsfield.dscp == 46"],
        hope: &[],
    },
    App { app: "dhcp", extra: "{}", to: None, expect: &["dhcp"], hope: &["dhcp.option.dhcp == 1"] },
    App { app: "arp", extra: "{}", to: Some("192.168.100.2"), expect: &["arp.opcode == 1", "arp.opcode == 2"], hope: &[] },
];

#[test]
fn one_pdu_per_application_is_valid_pcap() {
    if !tools_present() {
        return;
    }
    let mut rep = Report::default();
    for a in APPS {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "services"})).unwrap();
        s.execute("sim.advance", json!({"seconds": 5})).unwrap();
        let mut params = json!({"from": "PC1", "app": a.app});
        if let Some(to) = a.to {
            params["to"] = json!(to);
        }
        for (k, v) in serde_json::from_str::<Value>(a.extra).unwrap().as_object().unwrap() {
            params[k] = v.clone();
        }
        if let Err(e) = s.execute("pdu.complex", params) {
            rep.failures.push(format!("{}: the PDU was refused: {e}", a.app));
            continue;
        }
        s.execute("sim.advance", json!({"seconds": 5})).unwrap();
        let label = format!("pdu {}", a.app);
        let (path, frames) = export(&mut s, &format!("pdu-{}.pcap", a.app), "ethernet");
        rep.check_file(&label, &path, frames, "ether", "pcap");
        rep.require(&label, &path, a.expect);
        rep.hope(&label, &path, a.hope);
    }
    rep.finish();
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
    let mut rep = Report::default();
    if frames == 0 {
        rep.failures.push("wifi: the air capture is empty".into());
    } else {
        rep.check_file("wifi", &path, frames, "ieee-802-11", "pcap");
        rep.hope(
            "wifi",
            &path,
            &["wlan.fc.type_subtype == 8", "wlan.fc.type_subtype == 11", "wlan.fc.type_subtype == 0", "eapol", "wlan.fc.type == 2"],
        );
    }
    rep.finish();
}
