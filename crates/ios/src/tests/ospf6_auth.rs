//! OSPFv3 authentication: an IPsec Authentication Header on every packet.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;
use netlabcraft_wire::Frame;
use netlabcraft_wire::ah::Ah;
use netlabcraft_wire::ipv6::{ETHERTYPE_IPV6, Ipv6};
use netlabcraft_wire::suite::Integ;

use super::Lab;

const KEY_A: &str = "0123456789abcdef0123456789abcdef";
const KEY_B: &str = "fedcba9876543210fedcba9876543210";
const SHA_KEY: &str = "0123456789abcdef0123456789abcdef01234567";

/// Two routers on one link, OSPFv3 area 0, the given lines typed on each interface and in each
/// OSPFv3 process.
fn pair(if_a: &str, if_b: &str, proc_a: &str, proc_b: &str) -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    for (r, id, ifl, pl) in [(r1, "1.1.1.1", if_a, proc_a), (r2, "2.2.2.2", if_b, proc_b)] {
        lab.run(
            r,
            &format!("en\nconf t\nipv6 unicast-routing\nipv6 router ospf 1\nrouter-id {id}\n{pl}\nexit\nint g0/0\nipv6 enable\nipv6 ospf 1 area 0\nno shut\n{ifl}\nend"),
        );
    }
    (lab, r1, r2)
}

fn neighbours(lab: &mut Lab, r: DeviceId) -> String {
    lab.run(r, "show ipv6 ospf neighbor")
}

#[test]
fn matching_ah_settings_form_an_adjacency_and_the_hellos_carry_a_verifiable_header() {
    let line = format!("ospfv3 authentication ipsec spi 256 md5 {KEY_A}");
    let (mut lab, r1, r2) = pair(&line, &line, "", "");
    assert!(neighbours(&mut lab, r1).contains("FULL") && neighbours(&mut lab, r2).contains("FULL"));
    lab.sim.run_for(&lab.t, 12_000_000);
    lab.sim.clear_trace();
    lab.sim.run_for(&lab.t, 12_000_000);
    let key = [0x01u8, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
    let mut seen = 0;
    for e in lab.sim.trace() {
        let Ok(f) = Frame::decode(&e.frame) else { continue };
        if f.ethertype != ETHERTYPE_IPV6 {
            continue;
        }
        let Ok(ip) = Ipv6::decode(&f.payload) else { continue };
        if ip.next_header != 51 {
            continue;
        }
        let inner = Ah::open_transport6(&ip, Integ::HmacMd5_96, &key, 256).expect("the header verifies under the configured key");
        assert_eq!(inner.next_header, 89);
        assert!(Ah::open_transport6(&ip, Integ::HmacMd5_96, &[9; 16], 256).is_err());
        seen += 1;
    }
    assert!(seen >= 2, "hellos from both routers carry AH: {seen}");
    let shown = lab.run(r1, "show ipv6 ospf");
    assert!(shown.contains("authentication: AH md5 with SPI 256"), "{shown}");
    let cfg = lab.run(r1, "show running-config");
    assert!(cfg.contains(&format!(" ospfv3 authentication ipsec spi 256 md5 {KEY_A}")), "{cfg}");
}

#[test]
fn a_missing_or_different_header_keeps_the_routers_apart_and_says_why() {
    let line = format!("ospfv3 authentication ipsec spi 256 md5 {KEY_A}");
    // Header against none.
    let (mut lab, r1, r2) = pair(&line, "", "", "");
    let n = neighbours(&mut lab, r1);
    assert!(!n.contains("FULL") && n.contains("Not a neighbour: 2.2.2.2") && n.contains("no OSPFv3 authentication"), "{n}");
    let n = neighbours(&mut lab, r2);
    assert!(!n.contains("FULL") && n.contains("no AH header"), "{n}");
    // Another key, another SPI, another algorithm.
    for (other, why) in [
        (format!("ospfv3 authentication ipsec spi 256 md5 {KEY_B}"), "does not verify"),
        (format!("ospfv3 authentication ipsec spi 257 md5 {KEY_A}"), "another SPI"),
        (format!("ospfv3 authentication ipsec spi 256 sha1 {SHA_KEY}"), "does not verify"),
    ] {
        let (mut lab, r1, _) = pair(&line, &other, "", "");
        let n = neighbours(&mut lab, r1);
        assert!(!n.contains("FULL") && n.contains(why), "{other}: {n}");
    }
}

#[test]
fn an_area_setting_covers_its_interfaces_until_one_says_null() {
    let area = format!("area 0 authentication ipsec spi 300 sha1 {SHA_KEY}");
    let (mut lab, r1, r2) = pair("", "", &area, &area);
    assert!(neighbours(&mut lab, r1).contains("FULL"));
    assert!(lab.run(r1, "show ipv6 ospf").contains("authentication: AH sha1 with SPI 300"));
    let cfg = lab.run(r1, "show running-config");
    assert!(cfg.contains(&format!(" area 0 authentication ipsec spi 300 sha1 {SHA_KEY}")), "{cfg}");
    // One interface opts out: the other end still wants the header.
    lab.run(r1, "conf t\nint g0/0\nospfv3 authentication null\nend");
    assert!(!neighbours(&mut lab, r2).contains("FULL"));
    assert!(lab.run(r1, "show running-config").contains(" ospfv3 authentication null"));
    lab.run(r1, "conf t\nint g0/0\nno ospfv3 authentication\nend");
    assert!(neighbours(&mut lab, r2).contains("FULL"));
    lab.run(r1, "conf t\nipv6 router ospf 1\nno area 0 authentication\nend");
    assert!(!neighbours(&mut lab, r1).contains("FULL"));
}

#[test]
fn the_commands_check_the_spi_the_algorithm_and_the_key() {
    let (mut lab, r1, _) = pair("", "", "", "");
    lab.run(r1, "conf t\nint g0/0");
    assert!(lab.try_run(r1, &format!("ospfv3 authentication ipsec spi 100 md5 {KEY_A}")).contains("SPI is 256"));
    assert!(lab.try_run(r1, &format!("ospfv3 authentication ipsec spi 256 des {KEY_A}")).contains("md5 or sha1"));
    assert!(lab.try_run(r1, "ospfv3 authentication ipsec spi 256 md5 abcd").contains("32 hexadecimal"));
    assert!(lab.try_run(r1, &format!("ospfv3 authentication ipsec spi 256 sha1 {KEY_A}")).contains("40 hexadecimal"));
    assert!(lab.try_run(r1, "ospfv3 authentication ipsec spi 256 md5 7 0123").contains("type 7"));
    assert!(lab.try_run(r1, &format!("ospfv3 authentication ipsec spi 256 md5 0 {KEY_A}")).is_empty());
    lab.run(r1, "end");
    assert!(lab.run(r1, "show running-config").contains(&format!("spi 256 md5 {KEY_A}")));
}
