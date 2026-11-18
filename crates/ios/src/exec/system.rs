//! System `show` commands: `show version`, `show inventory`, `show environment`, `show processes`,
//! `show memory`, `show clock detail`, `show interfaces switchport` and `show tech-support`.
//! The figures are small and plausible and are worked out from what the device holds and from
//! the traffic it has handled; the layouts are NetLabCraft's own.

use std::fmt::Write as _;

use netlabcraft_model::{Device, DeviceKind, Topology, ifname};
use netlabcraft_sim::Sim;

use super::{Ctx, R};
use crate::grammar::{Id, Spec, Tok};
use crate::render;

/// Flash memory of every router and switch, in bytes (the figure `show flash` and
/// `show file systems` use).
pub(crate) const FLASH_BYTES: u64 = 268_435_456;
/// NVRAM, in bytes.
pub(crate) const NVRAM_BYTES: u64 = 262_144;

/// Main memory in kilobytes by kind of device.
fn main_memory_kb(d: &Device) -> u64 {
    match d.kind {
        DeviceKind::Router => 512 * 1024,
        DeviceKind::Switch => 128 * 1024,
        _ => 64 * 1024,
    }
}

/// `1 day, 2 hours, 3 minutes, 4 seconds`, leaving out what is zero.
fn span(us: u64) -> String {
    let secs = us / 1_000_000;
    let parts = [(secs / 86_400, "day"), (secs / 3_600 % 24, "hour"), (secs / 60 % 60, "minute"), (secs % 60, "second")];
    let text: Vec<String> = parts.iter().filter(|(n, _)| *n > 0).map(|(n, w)| format!("{n} {w}{}", if *n == 1 { "" } else { "s" })).collect();
    if text.is_empty() { "0 seconds".to_string() } else { text.join(", ") }
}

/// Frames the device has moved through all its ports since the simulation began.
fn activity(sim: &Sim, d: &Device) -> u64 {
    (0..d.ports.len()).fold(0u64, |acc, i| {
        let c = sim.port_counters(d.id, i);
        acc.saturating_add(c.in_packets()).saturating_add(c.out_packets())
    })
}

/// The share of the processor the device's work takes, in percent: a small base for the
/// control plane, more with the routing protocols running and with the frames per second seen.
fn cpu_percent(sim: &Sim, d: &Device) -> u64 {
    let protocols = u64::from(d.ospf.is_some()) + u64::from(d.rip.is_some()) + u64::from(d.eigrp.is_some());
    let secs = (sim.uptime_us(d.id) / 1_000_000).max(1);
    let rate = activity(sim, d) / secs;
    (1 + protocols + rate / 50).min(95)
}

/// The tasks the device runs, in the order `show processes` lists them.
fn tasks(d: &Device) -> Vec<&'static str> {
    let mut tasks: Vec<&str> = vec!["Console", "IP input", "ARP"];
    if d.kind == DeviceKind::Switch {
        tasks.extend(["Spanning tree", "MAC address table"]);
    }
    if !d.cdp_off && d.kind != DeviceKind::Pc {
        tasks.push("CDP");
    }
    if d.lldp_on {
        tasks.push("LLDP");
    }
    if d.ospf.is_some() {
        tasks.push("OSPF");
    }
    if d.rip.is_some() {
        tasks.push("RIP");
    }
    if d.eigrp.is_some() {
        tasks.push("EIGRP");
    }
    if !d.dhcp_pools.is_empty() {
        tasks.push("DHCP server");
    }
    if !d.ntp_servers.is_empty() {
        tasks.push("NTP client");
    }
    tasks
}

/// Run time of task number `n` in milliseconds: a share of the uptime that shrinks down the list.
fn run_time_ms(sim: &Sim, d: &Device, n: usize) -> u64 {
    sim.uptime_us(d.id) / 1_000 / 1_000 / (n as u64 + 2)
}

/// `show processes [cpu [sorted|history]]`.
fn processes(sim: &Sim, d: &Device, spec: Option<&Spec>) -> String {
    let has = |w: &str| spec.is_some_and(|sp| sp.path.iter().any(|t| matches!(t, Tok::K(k) if *k == w)));
    let busy = cpu_percent(sim, d);
    let one = busy.saturating_sub(busy / 5).max(1);
    let five = busy.saturating_sub(busy / 3).max(1);
    if has("history") {
        let mut s = String::from(
            "CPU use over time (percent of the processor; the simulator keeps no samples, so every column shows the figure worked out from the traffic so far)\n",
        );
        for (label, v) in [("last 60 seconds", busy), ("last 60 minutes", one), ("last 72 hours", five)] {
            let _ = writeln!(s, "  {label:<16} {v:>3}%  {}", "#".repeat(usize::try_from(v.min(50)).unwrap_or(0)));
        }
        let _ = write!(s, "Peak since the device started: {}%", busy.max(one).max(five));
        return s;
    }
    let mut list: Vec<(usize, &str)> = tasks(d).into_iter().enumerate().collect();
    if has("sorted") {
        list.sort_by_key(|(n, _)| std::cmp::Reverse(run_time_ms(sim, d, *n)));
    }
    let mut s = format!("CPU use: {busy}% over the last 5 s, {one}% over 1 min, {five}% over 5 min\n\n");
    let _ = writeln!(s, "{:>4}  {:<20} {:>12} {:>6}", "Task", "Name", "Run time (ms)", "Share");
    let total: u64 = (0..list.len()).map(|n| run_time_ms(sim, d, n)).sum::<u64>().max(1);
    for (n, name) in list {
        let t = run_time_ms(sim, d, n);
        let _ = writeln!(s, "{:>4}  {:<20} {:>12} {:>5}%", n + 1, name, t, t * busy / total);
    }
    s.trim_end().to_string()
}

/// Kilobytes the device's configuration and tables take in main memory.
fn used_kb(sim: &Sim, d: &Device) -> u64 {
    let config = u64::try_from(render::running_config(d).len()).unwrap_or(0) / 1024 + 1;
    let (arp, macs) = sim.state(d.id).map_or((0, 0), |st| (st.arp.len() as u64, st.macs.len() as u64));
    24 * 1024 + config * 4 + d.ports.len() as u64 * 32 + d.routes.len() as u64 / 4 + arp / 8 + macs / 16
}

/// `show memory [statistics]`.
fn memory(sim: &Sim, d: &Device) -> String {
    let total = main_memory_kb(d);
    let used = used_kb(sim, d);
    let free = total.saturating_sub(used);
    let config = u64::try_from(render::running_config(d).len()).unwrap_or(0) / 1024 + 1;
    let (arp, macs) = sim.state(d.id).map_or((0, 0), |st| (st.arp.len() as u64, st.macs.len() as u64));
    let mut s = format!("{:<12} {:>10} {:>10} {:>10}\n{:<12} {:>10} {:>10} {:>10}\n", "", "Total", "Used", "Free", "Main memory", total, used, free);
    let _ = writeln!(s, "Flash: {} bytes in use of {FLASH_BYTES}", flash_used(d));
    let _ = writeln!(s, "\nLargest free block: {} KB; lowest free so far: {} KB", free * 9 / 10, free * 8 / 10);
    let _ = writeln!(s, "Taken by the configuration: {} KB; routes: {}; ARP entries: {arp}; MAC entries: {macs}", config * 4, d.routes.len());
    s.push_str("(kilobytes)");
    s
}

/// Bytes of flash the files take.
fn flash_used(d: &Device) -> u64 {
    super::mgmt::flash_files(d).iter().map(|f| f.1).sum()
}

/// `show processes memory`.
fn processes_memory(sim: &Sim, d: &Device) -> String {
    let used = used_kb(sim, d) * 1024;
    let list = tasks(d);
    let each = used / (list.len() as u64 + 3).max(1);
    let mut s = format!("Main memory in use: {used} bytes of {} bytes\n\n", main_memory_kb(d) * 1024);
    let _ = writeln!(s, "{:>4}  {:<20} {:>12}", "Task", "Name", "Holding");
    for (n, name) in list.iter().enumerate() {
        let _ = writeln!(s, "{:>4}  {:<20} {:>12}", n + 1, name, each / (n as u64 + 1) + 4_096);
    }
    s.trim_end().to_string()
}

/// The room temperature the device sits in: the lab's environment when it is on, else a mild
/// constant.
fn room_temperature(t: &Topology) -> i32 {
    if t.environment.enabled { t.environment.temperature } else { 22 }
}

/// Inlet and inside temperatures in degrees Celsius: the inside runs warmer with more traffic.
fn temperatures(t: &Topology, sim: &Sim, d: &Device) -> (i32, i32) {
    let inlet = room_temperature(t);
    let load = i32::try_from(cpu_percent(sim, d) / 10).unwrap_or(0);
    (inlet, inlet + 8 + load)
}

/// `show environment [all|power|temperature]`.
fn environment(t: &Topology, sim: &Sim, d: &Device, spec: Option<&Spec>) -> String {
    let has = |w: &str| spec.is_some_and(|sp| sp.path.iter().any(|t| matches!(t, Tok::K(k) if *k == w)));
    let all = spec.is_none() || has("all");
    let power = all || has("power") || !has("temperature");
    let temp = all || has("temperature") || !has("power");
    let mut blocks: Vec<String> = Vec::new();
    if power {
        let mut s = String::from("Power\n");
        let bays = d.psus();
        if bays.is_empty() {
            let _ = writeln!(s, "  This model takes its power from an external adapter: {}", if d.powered_off { "no power" } else { "OK" });
        } else {
            for (i, fitted) in bays.iter().enumerate() {
                let state = if !fitted {
                    "empty bay"
                } else if d.powered_off {
                    "fitted, no output"
                } else {
                    "OK"
                };
                let _ = writeln!(s, "  Power supply {}: {state}", i + 1);
            }
            let working = bays.iter().filter(|f| **f).count();
            let _ = writeln!(s, "  Redundancy: {}", if working >= 2 { "one supply can fail without a restart" } else { "no spare supply" });
        }
        blocks.push(s.trim_end().to_string());
    }
    if temp {
        let (inlet, inside) = temperatures(t, sim, d);
        let state = if inside >= 70 {
            "critical"
        } else if inside >= 55 {
            "warm"
        } else {
            "normal"
        };
        let mut s = format!("Temperature\n  Air inlet: {inlet} C\n  Inside the chassis: {inside} C ({state}; warning at 55 C, shutdown at 70 C)");
        let _ = write!(s, "\n  Fans: {}", if d.powered_off { "stopped" } else { "running" });
        if t.environment.enabled {
            s.push_str("\n  The inlet follows the temperature of the lab's environment");
        }
        blocks.push(s);
    }
    blocks.join("\n\n")
}

/// `show inventory`.
fn inventory(d: &Device) -> String {
    let m = d.catalogue_model();
    let mut s = format!("NAME: \"{}\", DESCR: \"{}\"\nPID: {}, VID: V01, SN: PC{:08X}", d.name, m.description, m.label, d.id.0);
    for slot in m.slots() {
        match d.module_in(slot) {
            Some(card) => {
                let _ = write!(
                    s,
                    "\n\nNAME: \"Slot {slot}\", DESCR: \"{}\"\nPID: {}, VID: V01, SN: CD{:08X}",
                    card.description,
                    card.label,
                    d.id.0 ^ 0x5a5a
                );
            }
            None => {
                let _ = write!(s, "\n\nNAME: \"Slot {slot}\", DESCR: \"empty bay\"");
            }
        }
    }
    for (i, fitted) in d.psus().iter().enumerate() {
        let _ = write!(
            s,
            "\n\nNAME: \"Power supply {}\", DESCR: {}",
            i + 1,
            if *fitted {
                format!("\"power supply unit\"\nPID: PSU-{}, VID: V01, SN: PS{:08X}", i + 1, d.id.0.wrapping_add(i as u32))
            } else {
                "\"empty bay\"".to_string()
            }
        );
    }
    s
}

/// `show version`.
fn version(sim: &Sim, d: &Device) -> String {
    let m = d.catalogue_model();
    let mut kinds: Vec<(String, usize)> = Vec::new();
    for p in &d.ports {
        let k = ifname::parse(&p.name).map_or("other", |n| n.kind).to_string();
        match kinds.iter_mut().find(|(n, _)| *n == k) {
            Some(e) => e.1 += 1,
            None => kinds.push((k, 1)),
        }
    }
    let ports: Vec<String> = kinds.iter().map(|(k, n)| format!("{n} {k}")).collect();
    let since = if sim.last_reload_reason(d.id).is_some() { "since the last restart" } else { "since the simulation started" };
    let mut s = format!(
        "NetLabCraft simulated {}, software {v}\nModel {} ({})\n{} has been up for {} ({since})\n",
        d.kind.label().to_lowercase(),
        m.label,
        m.description,
        d.name,
        span(sim.uptime_us(d.id)),
        v = env!("CARGO_PKG_VERSION"),
    );
    let _ = writeln!(s, "Last restart: {}", sim.last_reload_reason(d.id).unwrap_or("power on (the simulation started)"));
    let _ = writeln!(s, "System image flash:{}", super::mgmt::boot_image(d));
    let _ = writeln!(s, "Memory: {} MB main, {} MB flash, {} KB NVRAM", main_memory_kb(d) / 1024, FLASH_BYTES / 1_048_576, NVRAM_BYTES / 1024);
    let _ = writeln!(s, "Ports: {} ({} in all)", ports.join(", "), d.ports.len());
    let _ = writeln!(s, "Base hardware address {}", d.ports.first().map_or("-".to_string(), |p| p.mac.to_string()));
    let _ = writeln!(s, "Licence: NetLabCraft teaching edition; every feature is unlocked and nothing expires");
    let _ = writeln!(s, "Configuration {}", if d.startup.is_some() { "saved (startup-config present)" } else { "not saved" });
    let _ = write!(s, "Configuration register 0x{:04x}", d.nvram.register());
    s
}

/// `show clock detail`: the time and where it came from.
fn clock_detail(c: &Ctx) -> String {
    let (now, set) = c.sim.clock(c.dev);
    let mut s = crate::clock::show(now, set);
    let source = match (c.sim.state(c.dev).and_then(|st| st.ntp_synced), set) {
        (Some(a), _) => format!("NTP server {a}"),
        (None, true) => "the console (clock set)".to_string(),
        (None, false) => "the simulation's own timer".to_string(),
    };
    let _ = write!(s, "\nTime source: {source}\nUptime: {}", span(c.sim.uptime_us(c.dev)));
    s
}

/// `show interfaces switchport`: every switch port in turn.
fn switchports(t: &Topology, d: &Device) -> R {
    if !d.switches_frames() {
        return Err("% Only switch ports have switchport settings".into());
    }
    let mut blocks = Vec::new();
    for i in 0..d.ports.len() {
        if d.is_switchport(i) {
            blocks.push(render::switchport(t, d, i)?);
        }
    }
    Ok(if blocks.is_empty() { "(no switch ports)".to_string() } else { blocks.join("\n\n") })
}

/// `show tech-support`: the main reports one after another, each cut at a bounded length.
fn tech_support(c: &Ctx) -> R {
    const SECTION_CAP: usize = 4_000;
    let (t, sim, d) = (&*c.t, &*c.sim, c.d()?);
    let mut sections: Vec<(&str, String)> = vec![
        ("show version", version(sim, d)),
        ("show inventory", inventory(d)),
        ("show environment all", environment(t, sim, d, None)),
        ("show processes cpu", processes(sim, d, None)),
        ("show memory statistics", memory(sim, d)),
        ("show running-config", render::running_config(d)),
        ("show ip interface brief", render::ip_int_brief(t, d)),
        ("show interfaces", render::interfaces(t, sim, d, None)),
    ];
    if d.switches_frames() {
        sections.push(("show interfaces trunk", render::trunks(t, d, None)));
        sections.push(("show vlan brief", render::vlans(t, d)));
    }
    let mut out = String::new();
    for (name, text) in sections {
        let cut: String = text.chars().take(SECTION_CAP).collect();
        let _ = write!(out, "------------------ {name} ------------------\n{cut}");
        if cut.len() < text.len() {
            out.push_str("\n(cut here: the section is longer than the support report keeps)");
        }
        out.push_str("\n\n");
    }
    Ok(out.trim_end().to_string())
}

/// Runs the system `show` commands.
pub(super) fn command(c: &mut Ctx, spec: &Spec, _args: &[String]) -> R {
    let has = |w: &str| spec.path.iter().any(|t| matches!(t, Tok::K(k) if *k == w));
    match spec.id {
        Id::ShowVersion => Ok(version(c.sim, c.d()?)),
        Id::ShowInventory => Ok(inventory(c.d()?)),
        Id::ShowEnv => Ok(environment(c.t, c.sim, c.d()?, Some(spec))),
        Id::ShowProcesses if has("memory") => Ok(processes_memory(c.sim, c.d()?)),
        Id::ShowProcesses => Ok(processes(c.sim, c.d()?, Some(spec))),
        Id::ShowMemory => Ok(memory(c.sim, c.d()?)),
        Id::ShowClock => Ok(clock_detail(c)),
        Id::ShowIntSwitchport => switchports(c.t, c.d()?),
        Id::ShowTechSupport => tech_support(c),
        _ => Err("% That command is not available here".into()),
    }
}
