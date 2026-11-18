//! IP SLA: sends the probes of the operations whose time has come and writes what happened
//! into the device (`netlabcraft_model::sla`), where tracked objects read it. The engine calls
//! [`step`] after every command; the clock only moves when something advances it, so an
//! operation that was due several times since the last step sends a few probes (at most
//! `MAX_CATCH_UP`), not one for every missed interval.

use netlabcraft_core::DeviceId;
use netlabcraft_model::Topology;
use netlabcraft_model::sla::{MAX_CATCH_UP, Probe, Start};
use netlabcraft_model::track::SlaOutcome;

use crate::{PingOptions, Probe as Answer, Sim, l3_ifaces};

/// Most probes one step sends in all (each can wait for its timeout in simulated time).
const MAX_PROBES_PER_STEP: u32 = 16;

/// One operation that is due.
struct Job {
    dev: DeviceId,
    id: u32,
    probe: Probe,
    timeout_us: u64,
    threshold_us: u64,
    runs: u32,
}

/// Sends the probes that are due. Returns whether an operation's last outcome changed, so the
/// caller refreshes what follows it.
pub fn step(sim: &mut Sim, t: &mut Topology) -> bool {
    let now = sim.now_us();
    let mut jobs: Vec<Job> = Vec::new();
    for d in t.devices.iter_mut() {
        if d.sla.ops.is_empty() {
            continue;
        }
        let dev = d.id;
        let sla = &mut d.sla;
        for (id, op) in &sla.ops {
            let (Some(probe), Some(schedule)) = (&op.probe, &op.schedule) else { continue };
            if schedule.start == Start::Pending {
                continue;
            }
            let Some(start) = sla.stats.get(id).and_then(|s| s.started_us) else { continue };
            if now < start {
                continue;
            }
            if let Some(life) = schedule.life_s
                && now >= start.saturating_add(u64::from(life).saturating_mul(1_000_000))
            {
                continue;
            }
            let every = u64::from(op.frequency_s).max(1).saturating_mul(1_000_000);
            let runs = match sla.stats.get(id).and_then(|s| s.last_run_us) {
                None => 1,
                Some(last) => u32::try_from(now.saturating_sub(last) / every).unwrap_or(MAX_CATCH_UP).min(MAX_CATCH_UP),
            };
            if runs > 0 {
                jobs.push(Job {
                    dev,
                    id: *id,
                    probe: probe.clone(),
                    timeout_us: u64::from(op.timeout_ms).saturating_mul(1_000),
                    threshold_us: u64::from(op.threshold_ms).saturating_mul(1_000),
                    runs,
                });
            }
        }
    }
    let mut changed = false;
    let mut budget = MAX_PROBES_PER_STEP;
    for job in jobs {
        let mut results = Vec::new();
        for _ in 0..job.runs {
            if budget == 0 {
                break;
            }
            budget -= 1;
            let at = sim.now_us();
            let (outcome, rtt) = run_probe(sim, &*t, &job);
            results.push((outcome, rtt, at));
        }
        let Some(d) = t.device_mut(job.dev) else { continue };
        let before = d.track.sla_outcome.get(&job.id).copied();
        for (outcome, rtt, at) in results {
            d.sla.stats.entry(job.id).or_default().record(outcome, rtt, at);
            d.track.sla_outcome.insert(job.id, outcome);
        }
        changed |= before != d.track.sla_outcome.get(&job.id).copied();
    }
    changed
}

/// One probe: how it ended and how long the answer took.
fn run_probe(sim: &mut Sim, t: &Topology, job: &Job) -> (SlaOutcome, Option<u64>) {
    let started = sim.now_us();
    let elapsed = |sim: &Sim| sim.now_us().saturating_sub(started);
    // The first echo to a neighbour can be spent resolving its address, so a probe that gets no
    // answer is sent once more before it counts as failed.
    let echo = |sim: &mut Sim, dest, source| {
        let opts = PingOptions { count: 1, timeout_us: job.timeout_us.max(1_000), source, ..PingOptions::default() };
        for _ in 0..2 {
            if let Ok(v) = sim.ping(t, job.dev, dest, opts)
                && let Some(Answer::Reply { rtt_us, .. }) = v.first()
            {
                return Some(*rtt_us);
            }
        }
        None
    };
    let rtt = match &job.probe {
        Probe::IcmpEcho { dest, source } => echo(sim, *dest, *source),
        Probe::TcpConnect { dest, port } => sim.tcp_connect(t, job.dev, *dest, *port).ok().map(|()| elapsed(sim)),
        Probe::UdpEcho { dest, .. } | Probe::UdpJitter { dest, .. } => {
            // The far end answers only with `ip sla responder`; the datagram's path is the echo's.
            let responder = t.devices.iter().any(|x| x.sla.responder && l3_ifaces(x).iter().any(|i| i.net.addr == *dest));
            if responder { echo(sim, *dest, None) } else { None }
        }
        Probe::HttpGet { dest, path } => match sim.http_get(t, job.dev, *dest, &dest.to_string(), path) {
            Ok((status, _)) if status.split_whitespace().nth(1).and_then(|c| c.parse::<u16>().ok()).is_some_and(|c| (200..400).contains(&c)) => {
                Some(elapsed(sim))
            }
            _ => None,
        },
    };
    match rtt {
        None => (SlaOutcome::Failed, None),
        Some(r) if r > job.threshold_us => (SlaOutcome::OverThreshold, Some(r)),
        Some(r) => (SlaOutcome::Ok, Some(r)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use netlabcraft_core::Ipv4Net;
    use netlabcraft_model::sla::{Op, Schedule};
    use netlabcraft_model::{DeviceKind, End};

    fn lab() -> (Topology, Sim, DeviceId, DeviceId) {
        let mut t = Topology::new();
        let r1 = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let r2 = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        t.connect(End { device: r1, port: 0 }, End { device: r2, port: 0 }).unwrap();
        for (d, ip) in [(r1, "10.0.0.1/30"), (r2, "10.0.0.2/30")] {
            let dev = t.device_mut(d).unwrap();
            dev.ports[0].ip = Some(ip.parse::<Ipv4Net>().unwrap());
            dev.ports[0].shutdown = false;
        }
        let mut sim = Sim::new();
        sim.topology_changed(&t);
        (t, sim, r1, r2)
    }

    fn schedule(t: &mut Topology, dev: DeviceId, id: u32, probe: Probe, now: u64) {
        let d = t.device_mut(dev).unwrap();
        d.sla.ops.insert(id, Op { probe: Some(probe), frequency_s: 10, timeout_ms: 1_000, ..Op::default() });
        d.sla.ops.get_mut(&id).unwrap().schedule = Some(Schedule { life_s: None, start: Start::Now });
        d.sla.stats.entry(id).or_default().started_us = Some(now);
    }

    #[test]
    fn an_echo_operation_probes_on_its_frequency_and_sees_a_cut() {
        let (mut t, mut sim, r1, r2) = lab();
        let dest = "10.0.0.2".parse().unwrap();
        schedule(&mut t, r1, 1, Probe::IcmpEcho { dest, source: None }, sim.now_us());
        assert!(step(&mut sim, &mut t), "the first outcome is a change");
        let d = t.device(r1).unwrap();
        assert_eq!(d.track.sla_outcome.get(&1), Some(&SlaOutcome::Ok));
        assert_eq!(d.sla.stats[&1].sent, 1);
        assert!(!step(&mut sim, &mut t), "not due yet");
        assert_eq!(t.device(r1).unwrap().sla.stats[&1].sent, 1);
        // The far router's port goes down; ten seconds later the probe fails.
        t.device_mut(r2).unwrap().ports[0].shutdown = true;
        sim.topology_changed(&t);
        sim.run_for(&t, 11_000_000);
        assert!(step(&mut sim, &mut t));
        let d = t.device(r1).unwrap();
        assert_eq!(d.track.sla_outcome.get(&1), Some(&SlaOutcome::Failed));
        assert_eq!((d.sla.stats[&1].sent, d.sla.stats[&1].failed), (2, 1));
    }

    #[test]
    fn a_pending_or_expired_schedule_sends_nothing() {
        let (mut t, mut sim, r1, _) = lab();
        let dest = "10.0.0.2".parse().unwrap();
        schedule(&mut t, r1, 1, Probe::IcmpEcho { dest, source: None }, sim.now_us());
        t.device_mut(r1).unwrap().sla.ops.get_mut(&1).unwrap().schedule = Some(Schedule { life_s: None, start: Start::Pending });
        assert!(!step(&mut sim, &mut t));
        t.device_mut(r1).unwrap().sla.ops.get_mut(&1).unwrap().schedule = Some(Schedule { life_s: Some(5), start: Start::Now });
        sim.run_for(&t, 6_000_000);
        assert!(!step(&mut sim, &mut t));
        assert_eq!(t.device(r1).unwrap().sla.stats[&1].sent, 0);
    }

    #[test]
    fn udp_operations_need_a_responder_and_tcp_needs_a_listener() {
        let (mut t, mut sim, r1, r2) = lab();
        let dest = "10.0.0.2".parse().unwrap();
        schedule(&mut t, r1, 1, Probe::UdpEcho { dest, port: 5000 }, sim.now_us());
        step(&mut sim, &mut t);
        assert_eq!(t.device(r1).unwrap().track.sla_outcome.get(&1), Some(&SlaOutcome::Failed));
        t.device_mut(r2).unwrap().sla.responder = true;
        sim.run_for(&t, 11_000_000);
        step(&mut sim, &mut t);
        assert_eq!(t.device(r1).unwrap().track.sla_outcome.get(&1), Some(&SlaOutcome::Ok));
        schedule(&mut t, r1, 2, Probe::TcpConnect { dest, port: 9 }, sim.now_us());
        step(&mut sim, &mut t);
        assert_eq!(t.device(r1).unwrap().track.sla_outcome.get(&2), Some(&SlaOutcome::Failed), "nothing listens on port 9");
    }
}
