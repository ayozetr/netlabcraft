//! Extended communities (RFC 4360) typed at the console: `set extcommunity rt|soo`, `send-community
//! extended`, `ip extcommunity-list` and `match extcommunity` decide what a path carries and which paths
//! a route map lets through.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

fn advance(lab: &mut Lab, secs: u64) {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
}

/// R1 (AS 65001) owns 172.16.1.0/24 and 172.16.3.0/24 and tags what it sends with the route map TAG:
/// the first prefix gets a route target and a site of origin, the other a different route target.
/// R2 (AS 65002) has `r2_extra` under `router bgp` and `r2_top` before it.
fn pair(r1_extra: &str, r2_top: &str, r2_extra: &str) -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.run(
        r1,
        &format!(
            "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nint lo1\nip add 172.16.1.1 255.255.255.0\nint lo3\nip add 172.16.3.1 255.255.255.0\nexit\n\
             ip prefix-list ONE seq 5 permit 172.16.1.0/24\n\
             route-map TAG permit 10\nmatch ip address prefix-list ONE\nset extcommunity rt 65001:100 soo 10.0.0.1:5\nexit\n\
             route-map TAG permit 20\nset extcommunity rt 65001:300\nexit\n\
             router bgp 65001\nbgp router-id 1.1.1.1\nneighbor 10.0.12.2 remote-as 65002\nneighbor 10.0.12.2 route-map TAG out\n\
             network 172.16.1.0 mask 255.255.255.0\nnetwork 172.16.3.0 mask 255.255.255.0\n{r1_extra}\nend"
        ),
    );
    lab.run(
        r2,
        &format!(
            "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nexit\n{r2_top}\n\
             router bgp 65002\nbgp router-id 2.2.2.2\nneighbor 10.0.12.1 remote-as 65001\n{r2_extra}\nend"
        ),
    );
    advance(&mut lab, 8);
    (lab, r1, r2)
}

/// The line of `show ip bgp` for a prefix.
fn row(lab: &mut Lab, dev: DeviceId, net: &str) -> String {
    lab.run(dev, "show ip bgp").lines().find(|l| l.contains(net)).unwrap_or_default().trim_start().to_string()
}

#[test]
fn extended_communities_travel_only_to_a_neighbour_with_send_community_extended() {
    let (mut lab, r1, r2) = pair("", "", "");
    let plain = lab.run(r2, "show ip bgp 172.16.1.0");
    assert!(!plain.contains("extended communities"), "no send-community extended: nothing is sent\n{plain}");
    // The standard kind does not carry them either.
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.12.2 send-community\nend");
    assert!(!lab.run(r2, "show ip bgp 172.16.1.0").contains("extended communities"));
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.12.2 send-community extended\nend");
    let one = lab.run(r2, "show ip bgp 172.16.1.0");
    assert!(one.contains("extended communities: RT:65001:100 SoO:10.0.0.1:5"), "{one}");
    let other = lab.run(r2, "show ip bgp 172.16.3.0");
    assert!(other.contains("extended communities: RT:65001:300"), "{other}");
    // `both` keeps the standard communities too, and the views and the configuration say which.
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.12.2 send-community both\nend");
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.12.2").contains("send-community both"));
    assert!(lab.run(r1, "show running-config").contains(" neighbor 10.0.12.2 send-community both\n"));
    lab.run(r1, "conf t\nrouter bgp 65001\nno neighbor 10.0.12.2 send-community standard\nend");
    assert!(lab.run(r1, "show running-config").contains(" neighbor 10.0.12.2 send-community extended\n"));
    lab.run(r1, "conf t\nrouter bgp 65001\nno neighbor 10.0.12.2 send-community\nend");
    assert!(!lab.run(r2, "show ip bgp 172.16.1.0").contains("extended communities"));
    assert!(!lab.run(r1, "show running-config").contains("send-community"));
}

#[test]
fn a_route_map_matches_extended_communities_through_a_list() {
    let top = "ip extcommunity-list standard WANT permit rt 65001:100\n\
               route-map IN permit 10\nmatch extcommunity WANT\nset local-preference 300\nexit\nroute-map IN permit 20\nexit";
    let (mut lab, r1, r2) = pair("neighbor 10.0.12.2 send-community extended", top, "neighbor 10.0.12.1 route-map IN in");
    let _ = r1;
    let first = row(&mut lab, r2, "172.16.1.0/24");
    let second = row(&mut lab, r2, "172.16.3.0/24");
    assert!(first.contains(" 300 "), "the route target matches: {first}");
    assert!(second.contains(" 100 ") && !second.contains(" 300 "), "{second}");
    // A deny clause on the same match drops only that prefix.
    lab.run(r2, "conf t\nroute-map IN deny 5\nmatch extcommunity WANT\nexit\nend");
    advance(&mut lab, 1);
    assert!(row(&mut lab, r2, "172.16.1.0/24").is_empty(), "{}", lab.run(r2, "show ip bgp"));
    assert!(row(&mut lab, r2, "172.16.3.0/24").starts_with('*'));
    // An expanded list reads the text of each community, and a standard line needs every value it names.
    lab.run(
        r2,
        "conf t\nroute-map IN deny 5\nno match extcommunity\nmatch extcommunity SITE\nexit\nip extcommunity-list expanded SITE permit SoO:10\nend",
    );
    advance(&mut lab, 1);
    assert!(row(&mut lab, r2, "172.16.1.0/24").is_empty() && row(&mut lab, r2, "172.16.3.0/24").starts_with('*'));
    lab.run(r2, "conf t\nip extcommunity-list standard BOTH permit rt 65001:100 soo 10.0.0.1:5\nroute-map IN deny 5\nno match extcommunity\nmatch extcommunity BOTH\nend");
    advance(&mut lab, 1);
    assert!(row(&mut lab, r2, "172.16.1.0/24").is_empty());
    lab.run(r2, "conf t\nno ip extcommunity-list standard BOTH permit rt 65001:100 soo 10.0.0.1:5\nend");
    advance(&mut lab, 1);
    assert!(row(&mut lab, r2, "172.16.1.0/24").starts_with('*'), "a match on a list that is gone matches nothing");
}

#[test]
fn lists_and_sets_are_shown_written_back_and_checked() {
    let top = "ip extcommunity-list standard WANT permit rt 65001:100 soo 10.0.0.1:5\nip extcommunity-list expanded SITE deny SoO:10\n\
               ip extcommunity-list expanded SITE permit .*\nroute-map IN permit 10\nmatch extcommunity WANT SITE\nset extcommunity rt 4200000001:7 additive\nexit";
    let (mut lab, r1, r2) = pair("", top, "");
    let shown = lab.run(r2, "show ip extcommunity-list");
    assert!(shown.contains("Extended community list WANT (standard)") && shown.contains("permit rt 65001:100 soo 10.0.0.1:5"), "{shown}");
    assert!(shown.contains("Extended community list SITE (expanded)") && shown.contains("deny SoO:10"), "{shown}");
    assert!(lab.run(r2, "show ip extcommunity-list WANT").lines().count() < shown.lines().count());
    assert!(lab.try_run(r2, "show ip extcommunity-list NOPE").starts_with('%'));
    let config = lab.run(r2, "show running-config");
    for line in [
        "ip extcommunity-list standard WANT permit rt 65001:100 soo 10.0.0.1:5\n",
        "ip extcommunity-list expanded SITE deny SoO:10\n",
        " match extcommunity WANT SITE\n",
        " set extcommunity rt 4200000001:7 additive\n",
    ] {
        assert!(config.contains(line), "{line}\n{config}");
    }
    assert!(lab.run(r2, "show route-map IN").contains("match extended community list: WANT, SITE"));
    // R1's route map, as the wire layer will carry it.
    assert!(lab.run(r1, "show running-config").contains(" set extcommunity rt 65001:100 soo 10.0.0.1:5\n"));
    lab.run(r2, "conf t");
    for bad in [
        "ip extcommunity-list standard X permit 65001:100",
        "ip extcommunity-list standard X permit rt bad",
        "ip extcommunity-list standard X permit rt 99999999999:1",
        "ip extcommunity-list standard X permit",
        "ip extcommunity-list expanded WANT permit .*",
        "ip extcommunity-list 600 permit rt 1:1",
    ] {
        let said = lab.try_run(r2, bad);
        assert!(said.contains('%'), "{bad}: {said}");
    }
    lab.run(r2, "route-map IN permit 10");
    for bad in
        ["set extcommunity", "set extcommunity 1:1", "set extcommunity rt", "set extcommunity soo 300.1.1.1:5", "match extcommunity exact-match"]
    {
        let said = lab.try_run(r2, bad);
        assert!(said.contains('%'), "{bad}: {said}");
    }
    lab.run(r2, "no set extcommunity\nno match extcommunity SITE\nexit\nno ip extcommunity-list expanded SITE\nno ip extcommunity-list standard WANT permit rt 65001:100 soo 10.0.0.1:5\nend");
    let config = lab.run(r2, "show running-config");
    assert!(
        !config.contains("extcommunity-list") && !config.contains("set extcommunity") && config.contains(" match extcommunity WANT\n"),
        "{config}"
    );
    lab.run(r2, "conf t\nrouter bgp 65002");
    assert!(lab.try_run(r2, "neighbor 10.0.12.1 send-community nonsense").contains('%'));
}
