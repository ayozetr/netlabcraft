//! How a device stamps and filters its log messages: `service timestamps`, `logging console`,
//! `logging buffered` and `logging trap`, taken from the configuration at each topology change.

use netlabcraft_model::Device;
use netlabcraft_model::mgmt::Timestamps;

/// One device's logging settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogCfg {
    pub ts_log: Option<Timestamps>,
    pub ts_debug: Option<Timestamps>,
    pub console: u8,
    pub buffered: u8,
    pub trap: u8,
    /// `logging monitor`: what terminal-monitor sessions see.
    pub monitor: u8,
    /// `service sequence-numbers`: every message carries its number.
    pub seq: bool,
    /// `logging rate-limit N [except LEVEL]`: messages a second, and the severity let through whatever the rate.
    pub rate: Option<(u32, u8)>,
}

impl Default for LogCfg {
    fn default() -> LogCfg {
        LogCfg { ts_log: None, ts_debug: None, console: 7, buffered: 7, trap: 6, monitor: 7, seq: false, rate: None }
    }
}

impl LogCfg {
    pub fn of(d: &Device) -> LogCfg {
        let m = &d.mgmt;
        LogCfg {
            ts_log: m.timestamps_log,
            ts_debug: m.timestamps_debug,
            console: m.logging_console.unwrap_or(7),
            buffered: m.logging_buffered.unwrap_or(7),
            trap: m.logging_trap.unwrap_or(6),
            monitor: m.logging_monitor.unwrap_or(7),
            seq: d.knobs.global.get("service sequence-numbers").is_some_and(|l| !l.starts_with("no ")),
            rate: rate_limit(d),
        }
    }
}

/// `logging rate-limit 10 [except errors]`: the limit and the severity that is never held back.
fn rate_limit(d: &Device) -> Option<(u32, u8)> {
    let line = d.knobs.global.get("logging rate-limit")?;
    let mut words = line.split_whitespace().skip(2).filter(|w| !matches!(*w, "all" | "console"));
    let limit: u32 = words.next()?.parse().ok().filter(|n| *n > 0)?;
    let except = match words.next() {
        Some("except") => words.next().and_then(netlabcraft_model::mgmt::severity_from).unwrap_or(0),
        _ => 0,
    };
    Some((limit, except))
}

/// The severity of a message: debug output is 7 (debugging), links going down and failures 3
/// (errors), everything else 5 (notifications).
pub fn severity(msg: &str) -> u8 {
    let l = msg.to_ascii_lowercase();
    if l.contains(" debug: ") || l.starts_with("debug") {
        7
    } else if l.contains("link down") || l.contains(" down") || l.contains("failed") || l.contains("violation") {
        3
    } else {
        5
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The calendar date of a day number since 1970-01-01 (public-domain civil-from-days algorithm
/// by Howard Hinnant).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The stamp before a message: the time of day by default; date and time (and milliseconds)
/// with `service timestamps … datetime [msec]`.
pub fn stamp(unix: i64, now_us: u64, ts: Option<Timestamps>) -> String {
    let secs = unix.rem_euclid(86_400);
    let hms = format!("{:02}:{:02}:{:02}", secs / 3_600, secs / 60 % 60, secs % 60);
    let Some(ts) = ts else { return format!("[{hms}]") };
    let (y, m, d) = civil_from_days(unix.div_euclid(86_400));
    let month = MONTHS.get(usize::try_from(m - 1).unwrap_or(0)).copied().unwrap_or("Jan");
    let ms = if ts.msec { format!(".{:03}", now_us / 1_000 % 1_000) } else { String::new() };
    format!("[{month} {d} {y} {hms}{ms}]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_and_severities() {
        // 2026-10-30 12:34:56 UTC.
        let t = 1_793_363_696;
        assert_eq!(stamp(t, 0, None), "[12:34:56]");
        assert_eq!(stamp(t, 7_250_000, Some(Timestamps { msec: true })), "[Oct 30 2026 12:34:56.250]");
        assert_eq!(stamp(t, 0, Some(Timestamps { msec: false })), "[Oct 30 2026 12:34:56]");
        assert_eq!(severity("ICMP debug: echo request"), 7);
        assert_eq!(severity("GigabitEthernet0/0: link down"), 3);
        assert_eq!(severity("Configuration changed from the console"), 5);
    }
}
