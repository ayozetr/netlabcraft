//! Routing-protocol and first-hop-redundancy settings that are kept and shown: the lines under
//! `router rip` and `router eigrp`, RIP and EIGRP options on an interface, and the HSRP and VRRP
//! settings of a group (two groups keep their own).

use netlabcraft_model::DeviceKind;

use super::Lab;

#[test]
fn settings_under_router_rip_and_router_eigrp_are_written_inside_their_blocks() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t\nrouter rip\nversion 2\nnetwork 10.0.0.0\nmaximum-paths 4\ndistance 100\nno validate-update-source");
    assert!(lab.try_run(r, "output-delay 5").starts_with("% "), "8-50");
    lab.run(
        r,
        "exit\nrouter eigrp 1\nnetwork 10.0.0.0\neigrp stub\neigrp stub connected summary\ndistance eigrp 80 170\neigrp log-neighbor-changes\nend",
    );
    let run = lab.run(r, "show running-config");
    let rip = run.split("router rip\n").nth(1).unwrap_or("").split("!\n").next().unwrap_or("");
    assert!(rip.contains(" maximum-paths 4\n") && rip.contains(" distance 100\n") && rip.contains(" no validate-update-source\n"), "{rip}");
    let eigrp = run.split("router eigrp 1\n").nth(1).unwrap_or("").split("!\n").next().unwrap_or("");
    assert!(
        eigrp.contains(" eigrp stub connected summary\n") && !eigrp.contains(" eigrp stub\n"),
        "the second stub line replaces the first: {eigrp}"
    );
    assert!(eigrp.contains(" distance eigrp 80 170\n") && eigrp.contains(" eigrp log-neighbor-changes\n"), "{eigrp}");
    lab.run(r, "conf t\nrouter eigrp 1\nno eigrp stub\nno distance eigrp\nrouter rip\nvalidate-update-source\nno distance\nend");
    let run = lab.run(r, "show running-config");
    assert!(!run.contains("eigrp stub") && !run.contains("distance") && !run.contains("validate-update-source"), "{run}");
    // IPv6 RIP and EIGRP processes have their own modes: the setting is refused there.
    lab.run(r, "conf t\nipv6 unicast-routing\nipv6 router rip LAB");
    assert!(lab.try_run(r, "output-delay 10").starts_with("% "), "not available under ipv6 router rip");
}

#[test]
fn interface_options_for_rip_eigrp_and_hsrp_groups_are_kept_per_number() {
    let mut lab = Lab::new();
    let r = lab.add(DeviceKind::Router);
    lab.run(r, "en\nconf t\nint g0/0\nip add 10.0.0.1 255.255.255.0\nno shut");
    lab.run(
        r,
        "ip rip send version 2\nip rip receive version 1 2\nip rip authentication mode md5\nip rip authentication key-chain LAB\nip rip v2-broadcast",
    );
    lab.run(r, "ip bandwidth-percent eigrp 1 50\nip bandwidth-percent eigrp 2 20\nip bandwidth-percent eigrp 1 60");
    lab.run(
        r,
        "standby 1 timers 1 3\nstandby 2 timers 2 6\nstandby 1 name WEB\nstandby 1 preempt delay minimum 30\nstandby 1 mac-address 0000.0c07.ac01",
    );
    lab.run(r, "standby use-bia\nvrrp 5 description primary\nstandby 1 authentication md5 key-string s3cret\nstandby 2 follow WEB");
    assert!(lab.try_run(r, "standby 300 name X").starts_with("% "), "0-255");
    assert!(lab.try_run(r, "standby 1 timers 1").starts_with("% "), "both times");
    lab.run(r, "end");
    let run = lab.run(r, "show running-config");
    for line in [
        " ip rip send version 2\n",
        " ip rip receive version 1 2\n",
        " ip rip authentication mode md5\n",
        " ip bandwidth-percent eigrp 1 60\n",
        " ip bandwidth-percent eigrp 2 20\n",
        " standby 1 timers 1 3\n",
        " standby 2 timers 2 6\n",
        " standby 1 name WEB\n",
        " standby 1 preempt delay minimum 30\n",
        " standby 1 mac-address 0000.0c07.ac01\n",
        " standby use-bia\n",
        " vrrp 5 description primary\n",
        " standby 2 follow WEB\n",
    ] {
        assert!(run.contains(line), "{line}: {run}");
    }
    assert!(!run.contains("eigrp 1 50"), "one setting per AS: {run}");
    lab.run(r, "conf t\nint g0/0\nno standby 1 timers\nno ip bandwidth-percent eigrp 2\nend");
    let run = lab.run(r, "show running-config");
    assert!(!run.contains("standby 1 timers") && run.contains(" standby 2 timers 2 6\n"), "{run}");
    assert!(!run.contains("eigrp 2 20") && run.contains(" ip bandwidth-percent eigrp 1 60\n"), "{run}");
}
