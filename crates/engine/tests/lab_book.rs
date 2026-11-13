//! Runs every lab of the lab book (`docs/labs/`) through the engine, exactly as written.

mod labbook;

use netlabcraft_engine::Session;
use serde_json::{Value, json};

struct Engine(Session);

impl labbook::Driver for Engine {
    fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.0.execute(id, params).map_err(|e| e.to_string())
    }

    fn type_lines(&mut self, device: &str, lines: &[String]) -> Result<Vec<String>, String> {
        lines
            .iter()
            .map(|l| {
                self.0
                    .execute("cli.exec", json!({"device": device, "line": l}))
                    .map(|r| r.get("output").and_then(Value::as_str).unwrap_or("").to_string())
                    .map_err(|e| e.to_string())
            })
            .collect()
    }
}

fn check(file: &str) {
    let lab = labbook::load(file);
    assert!(!lab.steps.is_empty(), "{file}: no steps");
    let fails = labbook::run(&lab, &mut Engine(Session::new()));
    assert!(fails.is_empty(), "{file} ({}):\n{}", lab.title, fails.join("\n"));
}

macro_rules! labs {
    ($($name:ident => $file:literal),* $(,)?) => {
        $(#[test] fn $name() { check($file); })*
        const LISTED: &[&str] = &[$($file),*];
    };
}

labs! {
    lab_01_device_basics => "01-device-basics.md",
    lab_02_dual_stack_addressing => "02-dual-stack-addressing.md",
    lab_03_vlsm_subnetting => "03-vlsm-subnetting.md",
    lab_04_arp_icmp_dhcp_simulation => "04-arp-icmp-dhcp-simulation.md",
    lab_05_vlans_and_trunks => "05-vlans-and-trunks.md",
    lab_06_inter_vlan_routing => "06-inter-vlan-routing.md",
    lab_07_stp_tuning => "07-stp-tuning.md",
    lab_08_etherchannel_lacp => "08-etherchannel-lacp.md",
    lab_09_dhcpv4_dhcpv6 => "09-dhcpv4-dhcpv6.md",
    lab_10_hsrp => "10-hsrp.md",
    lab_11_switch_security => "11-switch-security.md",
    lab_12_wlan_controller => "12-wlan-controller.md",
    lab_13_static_floating_routes => "13-static-floating-routes.md",
    lab_14_ospf => "14-ospf.md",
    lab_15_acls => "15-acls.md",
    lab_16_nat_pat => "16-nat-pat.md",
    lab_17_gre_over_ipsec => "17-gre-over-ipsec.md",
    lab_18_qos => "18-qos.md",
    lab_19_network_management => "19-network-management.md",
    lab_20_restconf => "20-restconf.md",
    lab_21_troubleshoot_vlans_trunks => "21-troubleshoot-vlans-trunks.md",
    lab_22_troubleshoot_routing => "22-troubleshoot-routing.md",
    lab_23_troubleshoot_ospf => "23-troubleshoot-ospf.md",
    lab_24_troubleshoot_acls => "24-troubleshoot-acls.md",
    lab_25_troubleshoot_dhcp => "25-troubleshoot-dhcp.md",
    lab_26_troubleshoot_nat => "26-troubleshoot-nat.md",
    lab_27_troubleshoot_stp_root => "27-troubleshoot-stp-root.md",
}

/// The cellular lab is not part of a CCNA course, so it has no number (the numbered labs 21 and up
/// are listed above, and `every_lab_file_has_a_test` counts only numbered files).
#[test]
fn lab_cellular_mobile_data() {
    check("cellular-mobile-data.md");
}

#[test]
fn every_lab_file_has_a_test() {
    assert_eq!(labbook::files(), LISTED.iter().map(|s| s.to_string()).collect::<Vec<_>>());
}

#[test]
fn the_runner_catches_a_refused_line_and_a_wrong_expectation() {
    let lab = labbook::parse(
        "x.md",
        "# X\n```lab\ndevice.add {\"model\": \"Router-2\", \"name\": \"R1\"}\n```\n```console R1\nenable\nfrobnicate\n```\n```verify\nR1# show ip interface brief\n  expect: Bogus0/9\nping R1 10.9.9.9 => success\n```\n",
    )
    .unwrap();
    let fails = labbook::run(&lab, &mut Engine(Session::new()));
    assert_eq!(fails.len(), 3, "{fails:#?}");
}
