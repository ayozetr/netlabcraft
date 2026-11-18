//! Scheduled reloads (`reload in N`, `reload at HH:MM`): the simulated time a device restarts.
//! The restart itself is the console layer's (`netlabcraft_ios::run_due_reloads`), because it
//! rebuilds the device from its saved configuration.

use netlabcraft_core::DeviceId;

use crate::Sim;

impl Sim {
    /// Schedules a reload `in_us` simulated microseconds from now; returns when it is due.
    pub fn schedule_reload(&mut self, dev: DeviceId, in_us: u64) -> u64 {
        let at = self.now.saturating_add(in_us);
        self.reloads.insert(dev, at);
        at
    }

    /// Cancels a scheduled reload; whether there was one.
    pub fn cancel_reload(&mut self, dev: DeviceId) -> bool {
        self.reloads.remove(&dev).is_some()
    }

    /// When a device's scheduled reload is due (simulated microseconds), if one is.
    pub fn reload_due_at(&self, dev: DeviceId) -> Option<u64> {
        self.reloads.get(&dev).copied()
    }

    /// The devices whose reload time has come, taken off the schedule.
    pub fn take_due_reloads(&mut self) -> Vec<DeviceId> {
        let now = self.now;
        let due: Vec<DeviceId> = self.reloads.iter().filter(|(_, at)| **at <= now).map(|(d, _)| *d).collect();
        for d in &due {
            self.reloads.remove(d);
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reload_comes_due_once_and_can_be_cancelled() {
        let mut sim = Sim::new();
        let d = DeviceId(1);
        assert!(sim.take_due_reloads().is_empty());
        let at = sim.schedule_reload(d, 0);
        assert_eq!(sim.reload_due_at(d), Some(at));
        assert_eq!(sim.take_due_reloads(), vec![d]);
        assert!(sim.take_due_reloads().is_empty());
        sim.schedule_reload(d, 60_000_000);
        assert!(sim.take_due_reloads().is_empty());
        assert!(sim.cancel_reload(d) && !sim.cancel_reload(d));
    }
}
