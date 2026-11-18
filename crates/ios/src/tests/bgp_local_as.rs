//! `neighbor … local-as N [no-prepend [replace-as [dual-as]]]` typed at the console: the AS number a
//! router presents to an external neighbour decides whether the session comes up and what the AS path
//! of what is sent and received looks like.

use netlabcraft_core::DeviceId;
use netlabcraft_model::DeviceKind;

use super::Lab;

fn advance(lab: &mut Lab, secs: u64) {
    lab.sim.run_for(&lab.t, secs * 1_000_000);
}

/// R1 (AS 65001, owns 172.16.1.0/24) - R2 (AS 65002, owns 172.16.2.0/24). `r1` and `r2` are lines added
/// under each `router bgp`; R2 expects `r2_remote` as the AS of R1.
fn pair(r1_extra: &str, r2_remote: u32, r2_extra: &str) -> (Lab, DeviceId, DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/1", r2, "g0/1");
    lab.run(
        r1,
        &format!(
            "en\nconf t\nint g0/1\nip add 10.0.12.1 255.255.255.252\nno shut\nint lo1\nip add 172.16.1.1 255.255.255.0\nexit\n\
             router bgp 65001\nbgp router-id 1.1.1.1\nneighbor 10.0.12.2 remote-as 65002\nnetwork 172.16.1.0 mask 255.255.255.0\n{r1_extra}\nend"
        ),
    );
    lab.run(
        r2,
        &format!(
            "en\nconf t\nint g0/1\nip add 10.0.12.2 255.255.255.252\nno shut\nint lo1\nip add 172.16.2.1 255.255.255.0\nexit\n\
             router bgp 65002\nbgp router-id 2.2.2.2\nneighbor 10.0.12.1 remote-as {r2_remote}\nnetwork 172.16.2.0 mask 255.255.255.0\n{r2_extra}\nend"
        ),
    );
    advance(&mut lab, 8);
    (lab, r1, r2)
}

/// The AS path column of the row for `net` in `show ip bgp`.
fn path_of(lab: &mut Lab, dev: DeviceId, net: &str) -> String {
    let table = lab.run(dev, "show ip bgp");
    let row = table.lines().find(|l| l.contains(net)).unwrap_or_default().to_string();
    let text = row.split("  ").last().unwrap_or_default().trim().to_string();
    // The last word is the origin code.
    text.rsplit_once(' ').map_or_else(String::new, |(path, _)| path.to_string())
}

#[test]
fn the_neighbour_has_to_expect_the_local_as_and_sees_it_in_the_path() {
    let (mut lab, r1, r2) = pair("neighbor 10.0.12.2 local-as 65050", 65050, "");
    assert!(lab.run(r2, "show ip bgp neighbors 10.0.12.1").contains("State: Established"));
    assert_eq!(path_of(&mut lab, r2, "172.16.1.0/24"), "65050 65001", "the local AS in front of the real one");
    let n = lab.run(r1, "show ip bgp neighbors 10.0.12.2");
    assert!(n.contains("Local AS 65050: it goes in front of this router's AS in the paths sent; it is put in front of the paths received"), "{n}");
    // What R1 receives starts with the local AS too.
    assert_eq!(path_of(&mut lab, r1, "172.16.2.0/24"), "65050 65002");
    assert!(lab.run(r1, "show running-config").contains(" neighbor 10.0.12.2 local-as 65050\n"));
}

#[test]
fn a_neighbour_that_expects_the_real_as_refuses_the_session() {
    let (mut lab, _r1, r2) = pair("neighbor 10.0.12.2 local-as 65050", 65001, "");
    let n = lab.run(r2, "show ip bgp neighbors 10.0.12.1");
    assert!(!n.contains("State: Established"), "{n}");
    assert!(!lab.run(r2, "show ip bgp").contains("172.16.1.0/24"));
}

#[test]
fn no_prepend_keeps_the_local_as_off_what_is_received_and_replace_as_hides_the_real_one() {
    let (mut lab, r1, r2) = pair("neighbor 10.0.12.2 local-as 65050 no-prepend", 65050, "");
    assert_eq!(path_of(&mut lab, r1, "172.16.2.0/24"), "65002", "no-prepend: received as it came");
    assert_eq!(path_of(&mut lab, r2, "172.16.1.0/24"), "65050 65001", "no-prepend does not change what is sent");
    lab.run(r1, "conf t\nrouter bgp 65001\nneighbor 10.0.12.2 local-as 65050 no-prepend replace-as\nend");
    advance(&mut lab, 8);
    assert_eq!(path_of(&mut lab, r2, "172.16.1.0/24"), "65050", "replace-as: the real AS is not in the path");
    let config = lab.run(r1, "show running-config");
    assert!(config.contains(" neighbor 10.0.12.2 local-as 65050 no-prepend replace-as\n"), "{config}");
    assert!(lab.run(r1, "show ip bgp neighbors 10.0.12.2").contains("it replaces this router's AS in the paths sent"));
}

#[test]
fn dual_as_lets_the_neighbour_use_either_number() {
    let (mut lab, r1, r2) = pair("neighbor 10.0.12.2 local-as 65050 no-prepend replace-as dual-as", 65001, "");
    assert!(lab.run(r2, "show ip bgp neighbors 10.0.12.1").contains("State: Established"));
    assert_eq!(path_of(&mut lab, r2, "172.16.1.0/24"), "65001", "the neighbour knows the real AS, so that is what it sees");
    lab.run(r2, "conf t\nrouter bgp 65002\nneighbor 10.0.12.1 remote-as 65050\nend");
    advance(&mut lab, 8);
    assert!(lab.run(r2, "show ip bgp neighbors 10.0.12.1").contains("State: Established"));
    assert_eq!(path_of(&mut lab, r2, "172.16.1.0/24"), "65050");
    assert!(lab.run(r1, "show running-config").contains(" neighbor 10.0.12.2 local-as 65050 no-prepend replace-as dual-as\n"));
}

#[test]
fn the_command_checks_its_words_and_can_be_removed() {
    let (mut lab, r1, r2) = pair("neighbor 10.0.12.2 local-as 65050", 65050, "");
    lab.run(r1, "conf t\nrouter bgp 65001");
    for bad in [
        "neighbor 10.0.12.2 local-as 65001",
        "neighbor 10.0.12.2 local-as 0",
        "neighbor 10.0.12.2 local-as 65050 replace-as",
        "neighbor 10.0.12.2 local-as 65050 no-prepend dual-as",
        "neighbor 10.0.12.9 local-as 65050",
    ] {
        let said = lab.try_run(r1, bad);
        assert!(said.contains('%'), "{bad}: {said}");
    }
    // An internal neighbour has no use for it.
    lab.run(r1, "neighbor 10.0.12.6 remote-as 65001");
    let said = lab.try_run(r1, "neighbor 10.0.12.6 local-as 65050");
    assert!(said.contains('%'), "{said}");
    lab.run(r1, "no neighbor 10.0.12.2 local-as\nend");
    lab.run(r2, "conf t\nrouter bgp 65002\nneighbor 10.0.12.1 remote-as 65001\nend");
    advance(&mut lab, 8);
    assert!(!lab.run(r1, "show running-config").contains("local-as"));
    assert_eq!(path_of(&mut lab, r2, "172.16.1.0/24"), "65001");
}
