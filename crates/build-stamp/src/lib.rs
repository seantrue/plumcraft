//! What a build was made from (B151), so two builds a person could confuse never read the same
//! in About: the git commit (short), whether the tree had uncommitted changes, and when it was
//! built. Captured by a build script ([`capture::emit`]), never by running git at run time.
//!
//! PlumCraft's own library is stamped here ([`shell`]). An application built on the shell stamps
//! its own crate the same way and hands its stamp to the shell's library (`plumcraft_ffi::app`):
//! its build script calls [`capture::emit`], and its code reads the values with [`stamp!`]. A
//! native shell's own program (the Swift app) passes its stamp in the core's config
//! (`app.built`). [`about`] puts them together as `app.about`'s `build`, with the line About
//! shows.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod capture;

use serde_json::{Value, json};

/// One build's facts. `commit` and `date` are "unknown" when the build couldn't say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    /// The short commit hash (7 hex digits as git gives it), or "unknown".
    pub commit: String,
    /// Whether tracked files had uncommitted changes; None: unknown.
    pub dirty: Option<bool>,
    /// When it was built, UTC, `YYYY-MM-DDTHH:MM:SSZ`, or "unknown".
    pub date: String,
}

const UNKNOWN: &str = "unknown";

/// The stamp the calling crate's build script set ([`capture::emit`]), read at compile time.
#[macro_export]
macro_rules! stamp {
    () => {
        $crate::Stamp::from_env(env!("PLUMCRAFT_STAMP_COMMIT"), env!("PLUMCRAFT_STAMP_DIRTY"), env!("PLUMCRAFT_STAMP_DATE"))
    };
}

/// PlumCraft's own library: the shell's source.
pub fn shell() -> Stamp {
    stamp!()
}

fn is_commit(s: &str) -> bool {
    (4..=40).contains(&s.len()) && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 20
        && b.iter().enumerate().all(|(k, c)| match k {
            4 | 7 => *c == b'-',
            10 => *c == b'T',
            13 | 16 => *c == b':',
            19 => *c == b'Z',
            _ => c.is_ascii_digit(),
        })
}

impl Stamp {
    /// From the three values a build script set; anything not in their form reads "unknown".
    pub fn from_env(commit: &str, dirty: &str, date: &str) -> Stamp {
        Stamp {
            commit: if is_commit(commit) { commit.to_ascii_lowercase() } else { UNKNOWN.into() },
            dirty: match dirty {
                "yes" => Some(true),
                "no" => Some(false),
                _ => None,
            },
            date: if is_date(date) { date.to_owned() } else { UNKNOWN.into() },
        }
    }

    /// From JSON a shell sent (`{commit, dirty, date}`): hostile input, so anything not in its
    /// form reads "unknown", and something that isn't an object at all is None.
    pub fn from_json(v: &Value) -> Option<Stamp> {
        let o = v.as_object()?;
        Some(Stamp::from_env(
            o.get("commit").and_then(Value::as_str).unwrap_or(""),
            match o.get("dirty").and_then(Value::as_bool) {
                Some(true) => "yes",
                Some(false) => "no",
                None => "",
            },
            o.get("date").and_then(Value::as_str).unwrap_or(""),
        ))
    }

    pub fn to_json(&self) -> Value {
        json!({"commit": self.commit, "dirty": self.dirty, "date": self.date})
    }

    /// The commit as About writes it: `22f5884`, `22f5884+changes`, `unknown`.
    pub fn id(&self) -> String {
        match self.dirty {
            Some(true) => format!("{}+changes", self.commit),
            _ => self.commit.clone(),
        }
    }
}

/// The line About shows, one a person can read out:
/// `Built 2026-11-24 21:51 UTC · shell 22f5884 · app 03d315b+changes`. The date is the newest
/// of the stamps' (the native program is built last, so normally its); `app` is there when an
/// application's library stamped itself; `ui` when the native program was built from another
/// commit, or another state of changes, than the shell's library.
pub fn line(shell: &Stamp, app: Option<&Stamp>, ui: Option<&Stamp>) -> String {
    let newest = [Some(shell), app, ui].into_iter().flatten().map(|s| s.date.as_str()).filter(|d| *d != UNKNOWN).max();
    let when = match newest {
        Some(d) => format!("{} {} UTC", &d[0..10], &d[11..16]),
        None => UNKNOWN.to_owned(),
    };
    let mut out = format!("Built {when} · shell {}", shell.id());
    if let Some(a) = app {
        out.push_str(&format!(" · app {}", a.id()));
    }
    if let Some(u) = ui
        && (u.commit != shell.commit || u.dirty != shell.dirty)
    {
        out.push_str(&format!(" · ui {}", u.id()));
    }
    out
}

/// `app.about`'s `build`: the line, and each stamp (`app` and `ui` null when there are none).
pub fn about(shell: &Stamp, app: Option<&Stamp>, ui: Option<&Stamp>) -> Value {
    json!({
        "line": line(shell, app, ui),
        "shell": shell.to_json(),
        "app": app.map(Stamp::to_json),
        "ui": ui.map(Stamp::to_json),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(commit: &str, dirty: &str, date: &str) -> Stamp {
        Stamp::from_env(commit, dirty, date)
    }

    #[test]
    fn this_build_is_stamped_in_form() {
        let me = shell();
        assert!(me.commit == UNKNOWN || is_commit(&me.commit), "{me:?}");
        assert!(is_date(&me.date), "{me:?}");
    }

    #[test]
    fn the_line_reads_out() {
        let sh = s("22f5884", "no", "2026-11-24T21:40:00Z");
        assert_eq!(line(&sh, None, None), "Built 2026-11-24 21:40 UTC · shell 22f5884");
        let app = s("03d315b", "yes", "2026-11-24T21:45:10Z");
        let ui = s("22f5884", "no", "2026-11-24T21:51:59Z");
        assert_eq!(line(&sh, Some(&app), Some(&ui)), "Built 2026-11-24 21:51 UTC · shell 22f5884 · app 03d315b+changes");
        // a native program built from another state: said
        let other = s("22f5884", "yes", "2026-11-24T22:00:00Z");
        assert_eq!(line(&sh, None, Some(&other)), "Built 2026-11-24 22:00 UTC · shell 22f5884 · ui 22f5884+changes");
        // nothing known
        let none = s("", "", "");
        assert_eq!(line(&none, None, None), "Built unknown · shell unknown");
    }

    #[test]
    fn two_builds_a_person_could_confuse_read_differently() {
        let a = s("22f5884", "yes", "2026-11-24T21:40:00Z");
        let b = s("22f5884", "yes", "2026-11-24T21:47:00Z");
        assert_ne!(line(&a, None, None), line(&b, None, None));
        let c = s("22f5884", "no", "2026-11-24T21:40:00Z");
        assert_ne!(line(&a, None, None), line(&c, None, None));
    }

    #[test]
    fn hostile_stamps_read_unknown() {
        for v in [
            json!({"commit": "/home/someone", "dirty": "yes", "date": "yesterday"}),
            json!({"commit": "g123456", "dirty": 1, "date": "2026-11-24 21:40:00"}),
            json!({"commit": "a".repeat(41), "date": "2026-11-24T21:40:00Z\n"}),
            json!({}),
        ] {
            let st = Stamp::from_json(&v).unwrap();
            assert_eq!((st.commit.as_str(), st.dirty, st.date.as_str()), (UNKNOWN, None, UNKNOWN), "{v}");
        }
        assert!(Stamp::from_json(&json!("22f5884")).is_none());
        assert!(Stamp::from_json(&json!(null)).is_none());
        let ok = Stamp::from_json(&json!({"commit": "ABCDEF0", "dirty": false, "date": "2026-11-24T21:40:00Z"})).unwrap();
        assert_eq!(ok, s("abcdef0", "no", "2026-11-24T21:40:00Z"));
    }

    #[test]
    fn dates_are_utc_in_form() {
        assert_eq!(capture::iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(capture::iso(1_795_816_260), "2026-11-27T21:51:00Z");
        assert_eq!(capture::iso(951_782_400), "2000-02-29T00:00:00Z");
        assert!(is_date(&capture::now()));
    }

    #[test]
    fn about_has_the_line_and_each_stamp() {
        let sh = s("22f5884", "no", "2026-11-24T21:40:00Z");
        let v = about(&sh, None, None);
        assert_eq!(v["line"], "Built 2026-11-24 21:40 UTC · shell 22f5884");
        assert_eq!(v["shell"], json!({"commit": "22f5884", "dirty": false, "date": "2026-11-24T21:40:00Z"}));
        assert!(v["app"].is_null() && v["ui"].is_null());
    }
}
