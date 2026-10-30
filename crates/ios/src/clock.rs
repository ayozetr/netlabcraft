//! Device clocks: `clock set` parsing and `show clock` text. UTC only.
//!
//! Calendar arithmetic uses the well-known days-from-civil conversion for the proleptic
//! Gregorian calendar (public-domain algorithms by Howard Hinnant).

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];

/// Days since 1970-01-01 of a calendar date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The calendar date of a day number since 1970-01-01.
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

fn days_in_month(y: i64, m: i64) -> i64 {
    days_from_civil(if m == 12 { y + 1 } else { y }, if m == 12 { 1 } else { m + 1 }, 1) - days_from_civil(y, m, 1)
}

/// `14:05:00 30 Oct 2026` or `14:05:00 Oct 30 2026` → Unix seconds.
pub fn parse_set(args: &str) -> Result<i64, String> {
    let w: Vec<&str> = args.split_whitespace().collect();
    let [time, a, b, year] = w.as_slice() else { return Err("% Usage: clock set hh:mm:ss <day> <month> <year>".into()) };
    let hms: Vec<i64> = time.split(':').map(|x| x.parse::<i64>().map_err(|_| format!("% `{time}` is not hh:mm:ss"))).collect::<Result<_, _>>()?;
    let [h, mi, s] = hms.as_slice() else { return Err(format!("% `{time}` is not hh:mm:ss")) };
    if !(0..24).contains(h) || !(0..60).contains(mi) || !(0..60).contains(s) {
        return Err(format!("% `{time}` is not a time of day"));
    }
    let month = |x: &str| MONTHS.iter().position(|m| x.len() >= 3 && m.eq_ignore_ascii_case(x.get(..3).unwrap_or(""))).map(|i| i as i64 + 1);
    let (day, mon) = match (a.parse::<i64>(), b.parse::<i64>()) {
        (Ok(d), Err(_)) => (d, month(b).ok_or_else(|| format!("% `{b}` is not a month"))?),
        (Err(_), Ok(d)) => (d, month(a).ok_or_else(|| format!("% `{a}` is not a month"))?),
        _ => return Err("% Give the day and the month's name (30 Oct or Oct 30)".into()),
    };
    let year: i64 = year.parse().ok().filter(|y| (1993..=2035).contains(y)).ok_or("% The year is 1993-2035")?;
    if day < 1 || day > days_in_month(year, mon) {
        return Err(format!("% {} has no day {day}", MONTHS.get((mon - 1) as usize).unwrap_or(&"")));
    }
    Ok(days_from_civil(year, mon, day) * 86_400 + h * 3_600 + mi * 60 + s)
}

/// `14:05:00 UTC Fri Oct 30 2026`, with a note when nobody set the clock.
pub fn show(unix: i64, set: bool) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let wd = DAYS.get(days.rem_euclid(7) as usize).unwrap_or(&"");
    let mon = MONTHS.get((m - 1) as usize).unwrap_or(&"");
    let mut s = format!("{:02}:{:02}:{:02} UTC {wd} {mon} {d} {y}", secs / 3_600, secs / 60 % 60, secs % 60);
    if !set {
        s.push_str("\n(not set: the clock started with the simulation; use `clock set`)");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_validates() {
        let t = parse_set("14:05:09 30 Oct 2026").unwrap();
        assert_eq!(parse_set("14:05:09 october 30 2026").unwrap(), t);
        assert_eq!(show(t, true), "14:05:09 UTC Fri Oct 30 2026");
        assert_eq!(show(packetcraft_sim::DEFAULT_CLOCK, false).lines().next().unwrap(), "00:00:00 UTC Thu Jan 1 2026");
        assert_eq!(show(parse_set("23:59:59 29 Feb 2028").unwrap(), true), "23:59:59 UTC Tue Feb 29 2028");
        assert!(parse_set("12:00:00 29 Feb 2027").is_err());
        assert!(parse_set("25:00:00 1 Jan 2026").is_err());
        assert!(parse_set("12:00 1 Jan 2026").is_err());
        assert!(parse_set("12:00:00 1 Foo 2026").is_err());
        assert!(parse_set("12:00:00 1 Jan 99999").is_err());
    }
}
