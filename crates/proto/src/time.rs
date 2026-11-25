//! Dates and times as a chart reads them (note 26 slice 8: time axes): ISO 8601 text to seconds
//! since 1970-01-01 UTC, and the ticks of a time axis (seconds, minutes, hours, days, months,
//! years) with their text. No time zone database: an offset written in the text is applied, a
//! time without one is read as UTC. Hostile text is `None`, never a panic.

/// Days from 1970-01-01 to `y-m-d` in the proleptic Gregorian calendar (Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `(year, month, day)` of a day count from 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn digits(s: &str, n: usize) -> Option<u32> {
    (s.len() == n && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

/// `HH:MM[:SS[.fff]]` as seconds into the day.
fn clock(t: &str) -> Option<f64> {
    let mut parts = t.splitn(3, ':');
    let h = digits(parts.next()?, 2)?;
    let mi = digits(parts.next()?, 2)?;
    let s = match parts.next() {
        None => 0.0,
        Some(s) => {
            let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
            let w = f64::from(digits(whole, 2)?);
            if frac.is_empty() {
                w
            } else if frac.len() <= 9 && frac.bytes().all(|b| b.is_ascii_digit()) {
                w + format!("0.{frac}").parse::<f64>().ok()?
            } else {
                return None;
            }
        }
    };
    (h <= 24 && mi < 60 && s < 61.0).then(|| f64::from(h) * 3600.0 + f64::from(mi) * 60.0 + s)
}

/// ISO 8601 text as seconds since 1970-01-01 UTC: `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM[:SS[.f]]`
/// (or a space for the `T`), with `Z` or `±HH:MM` optional; a bare `HH:MM[:SS]` as seconds into
/// a day. Anything else: `None`.
pub fn parse_iso(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.len() > 40 {
        return None;
    }
    if t.len() >= 5 && t.as_bytes().get(2) == Some(&b':') {
        return clock(t);
    }
    let (date, rest) = match t.find(['T', ' ']) {
        Some(i) => (&t[..i], Some(&t[i + 1..])),
        None => (t, None),
    };
    let mut d = date.splitn(3, '-');
    let y = digits(d.next()?, 4)?;
    let m = digits(d.next()?, 2)?;
    let day = digits(d.next()?, 2)?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    let mut secs = days_from_civil(i64::from(y), m, day) as f64 * 86_400.0;
    if let Some(rest) = rest {
        let (time, offset) = if let Some(x) = rest.strip_suffix('Z') {
            (x, 0.0)
        } else if let Some(i) = rest.rfind(['+', '-']).filter(|i| *i >= 5) {
            let (hh, mm) = rest[i + 1..].split_once(':').unwrap_or((&rest[i + 1..], "00"));
            let off = f64::from(digits(hh, 2)?) * 3600.0 + f64::from(digits(mm, 2)?) * 60.0;
            (&rest[..i], if rest.as_bytes()[i] == b'+' { off } else { -off })
        } else {
            (rest, 0.0)
        };
        secs += clock(time)? - offset;
    }
    Some(secs)
}

/// A column type that names a time (`DATE`, `TIME`, `TIMESTAMP`, `DATETIME`, any case).
pub fn is_time_type(ty: &str) -> bool {
    let t = ty.to_ascii_uppercase();
    t == "DATE" || t == "TIME" || t.starts_with("TIMESTAMP") || t == "DATETIME"
}

const MINUTE: f64 = 60.0;
const HOUR: f64 = 3600.0;
const DAY: f64 = 86_400.0;

/// Ticks for a time axis from `lo` to `hi` seconds, about one each `px_per` points over `len`:
/// whole seconds, minutes, hours, days, months or years, each with its text (`2026`, `Mar 2026`,
/// `3 Mar`, `14:30`, `14:30:05`). At most 13.
pub fn ticks(lo: f64, hi: f64, len: f64, px_per: f64) -> Vec<(f64, String)> {
    if !(lo.is_finite() && hi.is_finite()) || hi <= lo || len <= 0.0 {
        return Vec::new();
    }
    let want = (len / px_per.max(1.0)).clamp(2.0, 12.0);
    let span = hi - lo;
    let step = span / want;
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let hm = |t: f64, secs: bool| {
        let s = t.rem_euclid(DAY) as i64;
        if secs { format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{:02}:{:02}", s / 3600, s / 60 % 60) }
    };
    let mut out = Vec::new();
    if step >= 300.0 * DAY {
        // years
        let years = [1i64, 2, 5, 10, 20, 50, 100, 200, 500, 1000].into_iter().find(|n| *n as f64 * 365.25 * DAY >= step).unwrap_or(1000);
        let (y0, ..) = civil_from_days((lo / DAY).floor() as i64);
        let mut y = y0.div_euclid(years) * years;
        while out.len() < 13 {
            let t = days_from_civil(y, 1, 1) as f64 * DAY;
            if t > hi {
                break;
            }
            if t >= lo {
                out.push((t, y.to_string()));
            }
            y += years;
        }
    } else if step >= 25.0 * DAY {
        // months
        let months = [1i64, 2, 3, 6].into_iter().find(|n| *n as f64 * 30.4 * DAY >= step).unwrap_or(6);
        let (y0, m0, _) = civil_from_days((lo / DAY).floor() as i64);
        let mut k = (y0 * 12 + i64::from(m0) - 1).div_euclid(months) * months;
        while out.len() < 13 {
            let (y, m) = (k.div_euclid(12), (k.rem_euclid(12) + 1) as u32);
            let t = days_from_civil(y, m, 1) as f64 * DAY;
            if t > hi {
                break;
            }
            if t >= lo {
                let name = MONTHS[(m - 1) as usize];
                out.push((t, if m == 1 || out.is_empty() { format!("{name} {y}") } else { name.to_owned() }));
            }
            k += months;
        }
    } else {
        let unit = [
            1.0,
            2.0,
            5.0,
            10.0,
            15.0,
            30.0,
            MINUTE,
            2.0 * MINUTE,
            5.0 * MINUTE,
            10.0 * MINUTE,
            15.0 * MINUTE,
            30.0 * MINUTE,
            HOUR,
            2.0 * HOUR,
            3.0 * HOUR,
            6.0 * HOUR,
            12.0 * HOUR,
            DAY,
            2.0 * DAY,
            7.0 * DAY,
            14.0 * DAY,
        ]
        .into_iter()
        .find(|u| *u >= step)
        .unwrap_or(14.0 * DAY);
        let mut t = (lo / unit).ceil() * unit;
        while t <= hi && out.len() < 13 {
            let text = if unit >= DAY {
                let (_, m, d) = civil_from_days((t / DAY).floor() as i64);
                format!("{d} {}", MONTHS[(m - 1) as usize])
            } else if unit >= MINUTE {
                if t.rem_euclid(DAY) == 0.0 {
                    let (_, m, d) = civil_from_days((t / DAY).floor() as i64);
                    format!("{d} {}", MONTHS[(m - 1) as usize])
                } else {
                    hm(t, false)
                }
            } else {
                hm(t, true)
            };
            out.push((t, text));
            t += unit;
        }
    }
    out
}

/// A time as the core writes it in a readout: the date, and the clock when it isn't midnight.
pub fn text(t: f64) -> String {
    if !t.is_finite() {
        return String::new();
    }
    let (y, m, d) = civil_from_days((t / DAY).floor() as i64);
    let s = t.rem_euclid(DAY);
    if s == 0.0 {
        format!("{y:04}-{m:02}-{d:02}")
    } else {
        let s = s as i64;
        format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_text_is_seconds_since_1970() {
        assert_eq!(parse_iso("1970-01-01"), Some(0.0));
        assert_eq!(parse_iso("2026-11-25"), Some(days_from_civil(2026, 11, 25) as f64 * 86_400.0));
        assert_eq!(parse_iso("1970-01-02T01:00:00Z"), Some(90_000.0));
        assert_eq!(parse_iso("1970-01-01 01:30"), Some(5400.0));
        assert_eq!(parse_iso("1970-01-01T02:00:00+01:00"), Some(3600.0));
        assert_eq!(parse_iso("12:00:30"), Some(43_230.0));
        assert_eq!(parse_iso("2024-02-29T00:00:00.5Z"), Some(days_from_civil(2024, 2, 29) as f64 * 86_400.0 + 0.5));
        for junk in ["", "2026", "2026-13-01", "2026-1-1", "hello", "2026-01-01Tzz", "99:99", "2026-01-01T10:00+xx:00", "２０２６-01-01"] {
            assert_eq!(parse_iso(junk), None, "{junk:?}");
        }
        assert_eq!(civil_from_days(days_from_civil(1999, 12, 31)), (1999, 12, 31));
        assert_eq!(text(90_000.0), "1970-01-02 01:00:00");
    }

    #[test]
    fn time_ticks_fall_on_whole_units() {
        let day = |y, m, d| days_from_civil(y, m, d) as f64 * DAY;
        let months = ticks(day(2026, 1, 10), day(2026, 12, 20), 600.0, 80.0);
        assert!(months.iter().all(|(t, _)| civil_from_days((t / DAY) as i64).2 == 1), "on the 1st");
        assert!(months.first().is_some_and(|(_, s)| s.ends_with("2026")));
        let years = ticks(day(1990, 1, 1), day(2030, 1, 1), 600.0, 80.0);
        assert!(years.iter().all(|(_, s)| s.len() == 4));
        let hours = ticks(0.0, DAY, 600.0, 80.0);
        assert!(hours.iter().any(|(_, s)| s == "03:00" || s == "06:00"), "{hours:?}");
        assert!(ticks(5.0, 5.0, 600.0, 80.0).is_empty() && ticks(f64::NAN, 1.0, 600.0, 80.0).is_empty());
        assert!(ticks(-1e15, 1e15, 600.0, 80.0).len() <= 13);
    }
}
