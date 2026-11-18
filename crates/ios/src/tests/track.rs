//! Object tracking: the `track` objects, tracked and permanent static routes, and HSRP and VRRP
//! groups that follow an object.

use netlabcraft_model::DeviceKind;

use super::Lab;

/// R1 and R2 on 10.0.0.0/30; R1's second port has no cable.
fn two_routers() -> (Lab, netlabcraft_core::DeviceId, netlabcraft_core::DeviceId) {
    let mut lab = Lab::new();
    let r1 = lab.add(DeviceKind::Router);
    let r2 = lab.add(DeviceKind::Router);
    lab.cable(r1, "g0/0", r2, "g0/0");
    lab.run(r1, "en\nconf t\nint g0/0\nip add 10.0.0.1 255.255.255.248\nno shut\nint g0/1\nip add 192.168.5.1 255.255.255.0\nno shut\nend");
    lab.run(r2, "en\nconf t\nint g0/0\nip add 10.0.0.2 255.255.255.248\nno shut\nend");
    (lab, r1, r2)
}

#[test]
fn objects_are_created_shown_and_written_back() {
    let (mut lab, r1, _) = two_routers();
    lab.run(r1, "conf t\ntrack 1 interface g0/0 line-protocol\ndelay down 10 up 20\nexit");
    lab.run(r1, "track 2 interface g0/1 ip routing\ntrack 3 ip route 10.9.0.0/16 reachability\nexit");
    lab.run(r1, "track 4 ip route 10.8.0.0 255.255.0.0 reachability\nexit\ntrack 5 ip sla 7 reachability\nexit");
    lab.run(r1, "track 6 list boolean or\nobject 1\nobject 2 not\nend");
    let show = lab.run(r1, "show track 1");
    assert!(show.contains("Track 1: interface GigabitEthernet0/0 line-protocol") && show.contains("State is Up"), "{show}");
    assert!(show.contains("Delay: up 20 s, down 10 s") && show.contains("list object 6"), "{show}");
    let all = lab.run(r1, "show track");
    for want in ["Track 2: interface GigabitEthernet0/1 ip routing", "ip route 10.9.0.0/16 reachability", "ip sla 7 reachability", "list boolean or"]
    {
        assert!(all.contains(want), "{all}");
    }
    assert!(all.contains("Member: object 2 (negated)"), "{all}");
    let run = lab.run(r1, "show running-config");
    for want in [
        "track 1 interface GigabitEthernet0/0 line-protocol\n delay up 20 down 10\n",
        "track 6 list boolean or\n object 1\n object 2 not\n",
        "track 4 ip route 10.8.0.0/16 reachability\n",
    ] {
        assert!(run.contains(want), "{run}");
    }
    lab.run(r1, "conf t\nno track 6\nno track 1\nend");
    assert!(lab.run(r1, "show track 1").contains("There is no tracked object 1"));
    assert!(!lab.run(r1, "show track").contains("list boolean"));
}

#[test]
fn commands_are_checked() {
    let (mut lab, r1, _) = two_routers();
    lab.run(r1, "conf t");
    assert!(lab.try_run(r1, "track 0 interface g0/0 line-protocol").starts_with("% "), "1-1000");
    assert!(lab.try_run(r1, "track 1 interface g9/9 line-protocol").starts_with("% "), "no such port");
    assert!(lab.try_run(r1, "track 1 interface g0/0 speed").starts_with("% "));
    assert!(lab.try_run(r1, "track 1 list boolean xor").starts_with("% "));
    assert!(lab.try_run(r1, "track 9").starts_with("% "), "an object that does not exist");
    assert!(lab.try_run(r1, "no track 9").starts_with("% "));
    lab.run(r1, "track 1 ip sla 1 state");
    assert!(lab.try_run(r1, "object 2").starts_with("% "), "only a list holds objects");
    assert!(lab.try_run(r1, "delay sideways 5").starts_with("% "));
    assert!(lab.try_run(r1, "delay up 999").starts_with("% "));
    lab.run(r1, "exit\ntrack 2 list boolean and");
    assert!(lab.try_run(r1, "object 2").starts_with("% "), "a list cannot hold itself");
    assert!(lab.prompt(r1).ends_with("(config-track)#"));
    assert!(lab.try_run(r1, "ip route 1.1.1.0 255.255.255.0 10.0.0.2 track 0").starts_with("% "));
    lab.run(r1, "exit");
    assert!(lab.prompt(r1).ends_with("(config)#"));
}

#[test]
fn a_static_route_follows_its_object() {
    let (mut lab, r1, _) = two_routers();
    // Object 1 watches the port with no cable (down); object 2 the working one.
    lab.run(r1, "conf t\ntrack 1 interface g0/1 line-protocol\ntrack 2 interface g0/0 line-protocol\nexit");
    lab.run(r1, "ip route 172.16.0.0 255.255.0.0 10.0.0.2 track 1\nip route 172.17.0.0 255.255.0.0 10.0.0.2 track 2\nend");
    let routes = lab.run(r1, "show ip route");
    assert!(!routes.contains("172.16.0.0"), "the route that follows a down object is not installed:\n{routes}");
    assert!(routes.contains("172.17.0.0"), "{routes}");
    let run = lab.run(r1, "show running-config");
    assert!(run.contains("ip route 172.16.0.0 255.255.0.0 10.0.0.2 track 1\n"), "{run}");
    // Typing the route again without `track` makes it unconditional; removing the object frees it too.
    lab.run(r1, "conf t\nip route 172.16.0.0 255.255.0.0 10.0.0.2\nend");
    assert!(lab.run(r1, "show ip route").contains("172.16.0.0"));
    lab.run(r1, "conf t\nip route 172.16.0.0 255.255.0.0 10.0.0.2 track 1\nno track 1\nend");
    assert!(lab.run(r1, "show ip route").contains("172.16.0.0"), "a deleted object no longer holds the route back");
}

#[test]
fn a_permanent_route_stays_in_the_table_when_its_interface_is_down() {
    let (mut lab, r1, _) = two_routers();
    lab.run(r1, "conf t\nint g0/2\nip add 192.168.6.1 255.255.255.0\nexit");
    lab.run(r1, "ip route 10.77.0.0 255.255.0.0 g0/2\nip route 10.78.0.0 255.255.0.0 g0/2 permanent\nend");
    let routes = lab.run(r1, "show ip route");
    assert!(!routes.contains("10.77.0.0"), "g0/2 is shut down:\n{routes}");
    assert!(routes.contains("10.78.0.0"), "{routes}");
    assert!(lab.run(r1, "show running-config").contains("ip route 10.78.0.0 255.255.0.0 GigabitEthernet0/2 permanent\n"));
}

#[test]
fn hsrp_and_vrrp_follow_an_object_by_number() {
    let (mut lab, r1, _) = two_routers();
    lab.run(
        r1,
        "conf t\ntrack 1 interface g0/1 line-protocol\nexit\nint g0/0\nstandby 1 ip 10.0.0.6\nstandby 1 priority 110\nstandby 1 track 1 decrement 25",
    );
    lab.run(r1, "vrrp 2 ip 10.0.0.5\nvrrp 2 track 1 decrement 30\nend");
    let run = lab.run(r1, "show running-config");
    assert!(run.contains(" standby 1 track 1 decrement 25\n") && run.contains(" vrrp 2 track 1 decrement 30\n"), "{run}");
    // g0/1 has no cable, so object 1 is down and the priority drops.
    let roles = lab.sim.hsrp_roles(&lab.t);
    let role = roles.get(&r1).and_then(|v| v.iter().find(|r| r.group == 1)).expect("an HSRP role");
    assert_eq!((role.priority, role.configured_priority), (85, 110));
    assert_eq!(role.tracked_down, vec!["1".to_string()]);
    let show = lab.run(r1, "show track 1");
    assert!(show.contains("HSRP group 1 on GigabitEthernet0/0") && show.contains("VRRP group 2 on GigabitEthernet0/0"), "{show}");
}
