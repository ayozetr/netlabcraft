//! `address-family ipv6` of `router bgp`: neighbours reached over IPv6 or IPv4, activated per family,
//! and what `show bgp ipv6 unicast summary` says about their sessions.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

fn advance(lab: &mut Lab, secs: u64) {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
}

/// R1 (AS 65001) - R2 (AS `r2_as`) with both IPv4 and IPv6 addresses on the link. `r1` and `r2` are
/// lines typed under each `router bgp`.
fn pair(r2_as: u32, r1: &str, r2: &str) -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let a = lab.add(DeviceKind::Router);
    let b = lab.add(DeviceKind::Router);
    lab.cable(a, "g0/1", b, "g0/1");
    lab.run(
        a,
        &format!(
            "en\nconf t\nipv6 unicast-routing\nint g0/1\nip add 10.0.12.1 255.255.255.252\nipv6 address 2001:db8:12::1/64\nno shut\nexit\n\
             router bgp 65001\nbgp router-id 1.1.1.1\n{r1}\nend"
        ),
    );
    lab.run(
        b,
        &format!(
            "en\nconf t\nipv6 unicast-routing\nint g0/1\nip add 10.0.12.2 255.255.255.252\nipv6 address 2001:db8:12::2/64\nno shut\nexit\n\
             router bgp {r2_as}\nbgp router-id 2.2.2.2\n{r2}\nend"
        ),
    );
    advance(&mut lab, 8);
    (lab, a, b)
}

const OVER_V6_1: &str = "neighbor 2001:db8:12::2 remote-as 65002\naddress-family ipv6\nneighbor 2001:db8:12::2 activate\nexit-address-family";
const OVER_V6_2: &str = "neighbor 2001:db8:12::1 remote-as 65001\naddress-family ipv6\nneighbor 2001:db8:12::1 activate\nexit-address-family";

fn row(lab: &mut Lab, dev: DeviceId, who: &str) -> String {
    lab.run(dev, "show bgp ipv6 unicast summary").lines().find(|l| l.starts_with(who)).unwrap_or_default().to_string()
}

#[test]
fn a_session_over_ipv6_comes_up_when_both_ends_name_each_other() {
    let (mut lab, a, b) = pair(65_002, OVER_V6_1, OVER_V6_2);
    for (dev, who) in [(a, "2001:db8:12::2"), (b, "2001:db8:12::1")] {
        let r = row(&mut lab, dev, who);
        assert!(r.contains("IPv6") && !r.contains("never"), "{r}");
        assert!(!r.contains("Idle") && !r.contains("Active") && !r.contains("Connect"), "{r}");
    }
    assert!(lab.run(a, "show bgp ipv6 summary").contains("Address family: IPv6 unicast"));
}

#[test]
fn a_wrong_remote_as_keeps_the_session_down_and_the_summary_says_idle() {
    let (mut lab, a, _b) = pair(65_099, OVER_V6_1, OVER_V6_2);
    let r = row(&mut lab, a, "2001:db8:12::2");
    assert!(r.contains("Idle"), "{r}");
}

#[test]
fn a_neighbour_that_is_not_there_stays_active() {
    let (mut lab, a, _b) = pair(65_002, OVER_V6_1, "");
    let r = row(&mut lab, a, "2001:db8:12::2");
    assert!(r.contains("Idle") || r.contains("Active"), "{r}");
}

#[test]
fn an_ipv4_session_can_carry_ipv6_prefixes_and_both_ends_have_to_activate_them() {
    let both_v4_1 = "neighbor 10.0.12.2 remote-as 65002\naddress-family ipv6\nneighbor 10.0.12.2 activate\nexit-address-family";
    let both_v4_2 = "neighbor 10.0.12.1 remote-as 65001\naddress-family ipv6\nneighbor 10.0.12.1 activate\nexit-address-family";
    let (mut lab, a, _b) = pair(65_002, both_v4_1, both_v4_2);
    let r = row(&mut lab, a, "10.0.12.2");
    assert!(r.contains("IPv4") && !r.contains("never") && !r.contains("not negotiated"), "{r}");
    let one_sided = "neighbor 10.0.12.1 remote-as 65001";
    let (mut lab, a, _b) = pair(65_002, both_v4_1, one_sided);
    let r = row(&mut lab, a, "10.0.12.2");
    assert!(r.contains("IPv6 not negotiated"), "{r}");
}

#[test]
fn the_ipv6_family_is_written_back_in_the_running_configuration() {
    let (mut lab, a, _b) = pair(65_002, OVER_V6_1, OVER_V6_2);
    let config = lab.run(a, "show running-config");
    assert!(config.contains(" neighbor 2001:db8:12::2 remote-as 65002\n"), "{config}");
    assert!(config.contains(" address-family ipv6\n  neighbor 2001:db8:12::2 activate\n exit-address-family\n"), "{config}");
    lab.run(a, "conf t\nrouter bgp 65001\naddress-family ipv6\nno neighbor 2001:db8:12::2 activate\nend");
    assert!(!lab.run(a, "show running-config").contains("address-family ipv6"));
    lab.run(a, "conf t\nrouter bgp 65001\nneighbor 2001:db8:12::2 shutdown\nneighbor 2001:db8:12::2 description far end\nend");
    let config = lab.run(a, "show running-config");
    assert!(config.contains(" neighbor 2001:db8:12::2 shutdown\n") && config.contains(" neighbor 2001:db8:12::2 description far end\n"), "{config}");
    lab.run(a, "conf t\nrouter bgp 65001\nno neighbor 2001:db8:12::2\nend");
    assert!(!lab.run(a, "show running-config").contains("2001:db8:12::2"));
}

#[test]
fn the_address_family_mode_has_its_own_prompt_and_exit_leaves_it_for_the_process() {
    let mut lab = Lab::new();
    let a = lab.add(DeviceKind::Router);
    let out = lab.run(a, "en\nconf t\nrouter bgp 65001\naddress-family ipv6 unicast\nexit-address-family\nneighbor 2001:db8::9 remote-as 65009\nend");
    assert!(!out.contains('%'), "{out}");
    lab.run(a, "conf t\nrouter bgp 65001\naddress-family ipv6\nexit\nbgp router-id 9.9.9.9\nend");
    assert!(lab.run(a, "show running-config").contains(" bgp router-id 9.9.9.9\n"));
}
