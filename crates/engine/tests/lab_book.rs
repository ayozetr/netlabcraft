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
    lab_28_troubleshoot_ospf_authentication => "28-troubleshoot-ospf-authentication.md",
    lab_29_troubleshoot_acl_icmp => "29-troubleshoot-acl-icmp.md",
    lab_30_inspect_spanning_tree => "30-inspect-spanning-tree.md",
    lab_31_read_the_ospf_database => "31-read-the-ospf-database.md",
    lab_32_eigrp => "32-eigrp.md",
    lab_33_ipv6_routing => "33-ipv6-routing.md",
    lab_34_troubleshoot_etherchannel => "34-troubleshoot-etherchannel.md",
    lab_35_troubleshoot_port_security => "35-troubleshoot-port-security.md",
    lab_36_bgp => "36-bgp.md",
    lab_37_bgp_prefix_lists_route_maps => "37-bgp-prefix-lists-route-maps.md",
    lab_38_port_mirroring => "38-port-mirroring.md",
    lab_39_redistribute_and_distribute_lists => "39-redistribute-and-distribute-lists.md",
    lab_40_bgp_communities_aggregation => "40-bgp-communities-aggregation.md",
    lab_41_pppoe => "41-pppoe.md",
    lab_42_bgp_route_reflectors => "42-bgp-route-reflectors.md",
    lab_43_pppoe_with_routers => "43-pppoe-with-routers.md",
    lab_44_secure_a_wireless_network => "44-secure-a-wireless-network.md",
    lab_45_ospf_stub_nssa_virtual_link => "45-ospf-stub-nssa-virtual-link.md",
    lab_46_route_tags_and_bgp_peer_groups => "46-route-tags-and-bgp-peer-groups.md",
    lab_47_bgp_passwords_and_conditional_default => "47-bgp-passwords-and-conditional-default.md",
    lab_48_dial_up => "48-dial-up.md",
    lab_49_ospf_authentication_rollover => "49-ospf-authentication-rollover-key-chains-ospfv3-ah.md",
    lab_50_ospf_adjacency_states_and_dr_bdr => "50-ospf-adjacency-states-and-dr-bdr.md",
    lab_51_ospf_flooding_retransmission_and_lsa_ageing => "51-ospf-flooding-retransmission-and-lsa-ageing.md",
    lab_52_ospf_nssa_only_type_7_lsas => "52-ospf-nssa-only-type-7-lsas.md",
    lab_53_ike_with_certificates_and_expiry => "53-ike-with-certificates-and-expiry.md",
    lab_54_pki_enrolment_by_hand_and_ca_grant => "54-pki-enrolment-by-hand-and-ca-grant.md",
    lab_56_peap_with_a_server_certificate_the_client_does_not_trust => "56-peap-with-a-server-certificate-the-client-does-not-trust.md",
    lab_57_bgp_prefix_limits_and_timers => "57-bgp-prefix-limits-and-timers.md",
    lab_58_aliases_reload_extended_ping_and_telnet_by_name => "58-aliases-reload-extended-ping-and-telnet-by-name.md",
    lab_61_troubleshoot_site_to_site_vpn => "61-troubleshoot-site-to-site-vpn.md",
    lab_62_troubleshoot_remote_access_vpn => "62-troubleshoot-remote-access-vpn.md",
    lab_63_troubleshoot_wireless_clients => "63-troubleshoot-wireless-clients.md",
    lab_64_troubleshoot_ip_phones => "64-troubleshoot-ip-phones.md",
    lab_65_debug_icmp_arp_and_packets => "65-debug-icmp-arp-and-packets.md",
    lab_66_debug_rip_and_eigrp => "66-debug-rip-and-eigrp.md",
    lab_67_debug_spanning_tree_root => "67-debug-spanning-tree-root.md",
    lab_68_troubleshoot_ipv6 => "68-troubleshoot-ipv6.md",
    lab_69_troubleshoot_hsrp => "69-troubleshoot-hsrp.md",
    lab_70_troubleshoot_dhcp_snooping => "70-troubleshoot-dhcp-snooping.md",
    lab_71_troubleshoot_acl_order => "71-troubleshoot-acl-order.md",
    lab_72_debug_nat => "72-debug-nat.md",
    lab_73_python_on_a_board => "73-python-on-a-board.md",
    lab_74_javascript_on_a_microcontroller => "74-javascript-on-a-microcontroller.md",
    lab_75_reverse_telnet_through_an_octal_card => "75-reverse-telnet-through-an-octal-card.md",
    lab_81_mst_regions => "81-mst-regions.md",
    lab_82_igmp_snooping => "82-igmp-snooping.md",
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
