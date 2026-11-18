//! IPv6 unicast prefixes over BGP: `network`, `redistribute`, `aggregate-address`, `ipv6 prefix-list`
//! and route maps in `address-family ipv6`, over sessions on IPv6 or on IPv4.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

fn advance(lab: &mut Lab, secs: u64) {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
}

/// R1 (AS 65001) - R2 (AS 65002). R1 owns the loopbacks 2001:db8:a:1::/64 and 2001:db8:a:2::/64, R2
/// owns 2001:db8:b::/64. `r1` and `r2` are lines typed in the config mode before `router bgp`, `a1`
/// and `a2` lines typed under `address-family ipv6`; `v4` runs the session over IPv4.
fn pair(v4: bool, r1: &str, a1: &str, r2: &str, a2: &str) -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let a = lab.add(DeviceKind::Router);
    let b = lab.add(DeviceKind::Router);
    lab.cable(a, "g0/1", b, "g0/1");
    let (n1, n2) = if v4 { ("10.0.12.2", "10.0.12.1") } else { ("2001:db8:12::2", "2001:db8:12::1") };
    lab.run(
        a,
        &format!(
            "en\nconf t\nipv6 unicast-routing\nint g0/1\nip add 10.0.12.1 255.255.255.252\nipv6 address 2001:db8:12::1/64\nno shut\nexit\n\
             int lo1\nipv6 address 2001:db8:a:1::1/64\nexit\nint lo2\nipv6 address 2001:db8:a:2::1/64\nexit\n{r1}\n\
             router bgp 65001\nbgp router-id 1.1.1.1\nneighbor {n1} remote-as 65002\naddress-family ipv6\nneighbor {n1} activate\n{a1}\nend"
        ),
    );
    lab.run(
        b,
        &format!(
            "en\nconf t\nipv6 unicast-routing\nint g0/1\nip add 10.0.12.2 255.255.255.252\nipv6 address 2001:db8:12::2/64\nno shut\nexit\n\
             int lo1\nipv6 address 2001:db8:b::1/64\nexit\n{r2}\n\
             router bgp 65002\nbgp router-id 2.2.2.2\nneighbor {n2} remote-as 65001\naddress-family ipv6\nneighbor {n2} activate\n{a2}\nend"
        ),
    );
    advance(&mut lab, 8);
    (lab, a, b)
}

/// The number of prefixes `dev` has received from `who`: the last column of its row.
fn received(lab: &mut Lab, dev: DeviceId, who: &str) -> String {
    let out = lab.run(dev, "show bgp ipv6 unicast summary");
    let row = out.lines().find(|l| l.starts_with(who)).unwrap_or_default().to_string();
    row.split_whitespace().last().unwrap_or_default().to_string()
}

fn paths(lab: &mut Lab, dev: DeviceId) -> String {
    let out = lab.run(dev, "show bgp ipv6 unicast summary");
    out.lines().find(|l| l.starts_with("Paths in the table")).unwrap_or_default().to_string()
}

#[test]
fn network_statements_are_exchanged_over_an_ipv6_session() {
    let (mut lab, a, b) = pair(false, "", "network 2001:db8:a:1::/64\nnetwork 2001:db8:a:2::/64", "", "network 2001:db8:b::/64");
    assert_eq!(received(&mut lab, a, "2001:db8:12::2"), "1");
    assert_eq!(received(&mut lab, b, "2001:db8:12::1"), "2");
    assert_eq!(paths(&mut lab, b), "Paths in the table: 3 (3 best) for 3 prefixes");
}

#[test]
fn a_network_that_is_not_in_the_routing_table_is_not_advertised() {
    let (mut lab, _a, b) = pair(false, "", "network 2001:db8:dead::/48", "", "");
    assert_eq!(received(&mut lab, b, "2001:db8:12::1"), "0");
}

#[test]
fn an_ipv4_session_carries_ipv6_prefixes_with_the_interface_address_as_the_next_hop() {
    let (mut lab, a, b) = pair(true, "", "network 2001:db8:a:1::/64", "", "network 2001:db8:b::/64");
    assert_eq!(received(&mut lab, b, "10.0.12.1"), "1");
    assert_eq!(received(&mut lab, a, "10.0.12.2"), "1");
}

#[test]
fn an_ipv6_prefix_list_filters_what_a_neighbour_sends() {
    let lists = "ipv6 prefix-list NO-A2 seq 5 deny 2001:db8:a:2::/64\nipv6 prefix-list NO-A2 seq 10 permit ::/0 le 128";
    let (mut lab, _a, b) =
        pair(false, "", "network 2001:db8:a:1::/64\nnetwork 2001:db8:a:2::/64", lists, "neighbor 2001:db8:12::1 prefix-list NO-A2 in");
    assert_eq!(received(&mut lab, b, "2001:db8:12::1"), "1");
    assert!(lab.run(b, "show ipv6 prefix-list NO-A2").contains("seq 5 deny 2001:db8:a:2::/64"));
    let config = lab.run(b, "show running-config");
    assert!(config.contains("ipv6 prefix-list NO-A2 seq 10 permit ::/0 le 128\n"), "{config}");
    assert!(config.contains("  neighbor 2001:db8:12::1 prefix-list NO-A2 in\n"), "{config}");
}

#[test]
fn a_route_map_matches_ipv6_prefix_lists_and_a_deny_clause_drops_the_prefix() {
    let maps =
        "ipv6 prefix-list A2 permit 2001:db8:a:2::/64\nroute-map OUT deny 10\nmatch ipv6 address prefix-list A2\nexit\nroute-map OUT permit 20\nexit";
    let (mut lab, a, b) =
        pair(false, maps, "network 2001:db8:a:1::/64\nnetwork 2001:db8:a:2::/64\nneighbor 2001:db8:12::2 route-map OUT out", "", "");
    assert_eq!(received(&mut lab, b, "2001:db8:12::1"), "1");
    let config = lab.run(a, "show running-config");
    assert!(config.contains(" match ipv6 address prefix-list A2\n"), "{config}");
    assert!(lab.run(a, "show route-map OUT").contains("match IPv6 prefix list: A2"));
}

#[test]
fn an_aggregate_with_summary_only_hides_the_more_specific_prefixes() {
    let (mut lab, _a, b) =
        pair(false, "", "network 2001:db8:a:1::/64\nnetwork 2001:db8:a:2::/64\naggregate-address 2001:db8:a::/48 summary-only", "", "");
    assert_eq!(received(&mut lab, b, "2001:db8:12::1"), "1");
    let (mut lab, _a, b) = pair(false, "", "network 2001:db8:a:1::/64\naggregate-address 2001:db8:a::/48", "", "");
    assert_eq!(received(&mut lab, b, "2001:db8:12::1"), "2", "without summary-only both go");
}

#[test]
fn redistributing_connected_routes_advertises_them_as_incomplete() {
    let (mut lab, _a, b) = pair(false, "", "redistribute connected", "", "");
    let n: usize = received(&mut lab, b, "2001:db8:12::1").parse().unwrap_or(0);
    assert!(n >= 3, "the loopbacks and the link are advertised: {n}");
}

#[test]
fn the_family_lines_are_written_back() {
    let (mut lab, a, _b) = pair(
        false,
        "",
        "network 2001:db8:a:1::/64\naggregate-address 2001:db8:a::/48 summary-only as-set\nredistribute connected\nmaximum-paths 2\n\
         neighbor 2001:db8:12::2 next-hop-self\nneighbor 2001:db8:12::2 send-community both\nneighbor 2001:db8:12::2 weight 50",
        "",
        "",
    );
    let config = lab.run(a, "show running-config");
    for line in [
        "  network 2001:db8:a:1::/64\n",
        "  aggregate-address 2001:db8:a::/48 summary-only as-set\n",
        "  redistribute connected\n",
        "  maximum-paths 2\n",
        "  neighbor 2001:db8:12::2 next-hop-self\n",
        "  neighbor 2001:db8:12::2 send-community both\n",
        "  neighbor 2001:db8:12::2 weight 50\n",
    ] {
        assert!(config.contains(line), "{line}: {config}");
    }
}
