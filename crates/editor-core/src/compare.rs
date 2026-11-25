//! Compare's exact difference |A − B| for a shell that draws natively (note 26 slice 2b): the
//! difference ([`crate::diff`]) computed on a thread of its own from the two pinned values' own
//! numbers, once per pair of values and mode, and its pictures kept by the media service under
//! handles of their own ([`plumcraft_media::MediaService::put`]), so a shell fetches them as it
//! fetches any picture (`media.get {handle, want: "rgba"}`, `plum_blob`). `compare.get` carries
//! it as `difference` while Compare shows the difference.
//!
//! Never waits: a side not read yet, or a difference being computed, is `{status: "pending"}`, and
//! a shell asks again. A side that isn't a picture of numbers is `{status: "failed", error}`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use plumcraft_media::{Fetch, Made, MediaService, Want};
use plumcraft_proto::Handle;
use plumcraft_proto::catalogue::Media;
use plumcraft_proto::data::{DataRef, Form};
use plumcraft_proto::display::Display;
use plumcraft_session::Session;
use plumcraft_session::run::WatchRole;
use plumcraft_session::viewer_cmds::{CompareHow, DiffMode};
use serde_json::{Value, json};

use crate::diff::{self, Diff};
use crate::plain::Color;
use crate::probe::Pixels;

/// The colour of a NaN difference: magenta, as the research software's viewer paints it.
pub const NAN_COLOUR: Color = Color::from_rgb(255, 0, 255);
/// The most differences kept (each pair and mode is one).
const MAX_KEPT: usize = 16;

enum Job {
    Working,
    Done(Value),
}

/// The differences asked for, by pair and mode.
#[derive(Default)]
pub struct Diffs {
    jobs: Arc<Mutex<HashMap<String, Job>>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A side's full value, the media read that gives its numbers, and its channel names.
fn side(d: Option<&Display>) -> Result<(DataRef, Want, Vec<String>), String> {
    let d = d.ok_or("not pinned, or no value yet")?;
    let full = d.full.clone().ok_or("no numbers to compare (no full value)")?;
    let names = match &full.meta.form {
        Form::Array { names, .. } => names.clone(),
        _ => Vec::new(),
    };
    let want = if d.media == Media::Image { Want::Numbers } else { Want::Bytes };
    Ok((full, want, names))
}

/// What the shell is told of a difference.
fn json_of(d: &Diff, by_handle: bool, key: &str, media: &MediaService) -> Value {
    let pictures: Vec<Value> = d
        .pictures
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let handle = Handle::raw(format!("plumcraft:compare:{key}:{k}").as_bytes());
            let r = DataRef { handle: Some(handle.clone()), meta: Default::default(), at: Vec::new() };
            media.put(&r, Want::Rendition, Made::Picture(Arc::new(p.picture.clone())));
            json!({"handle": handle.to_string(), "caption": p.caption, "w": p.picture.w, "h": p.picture.h, "top": crate::viewers::number(p.top, false)})
        })
        .collect();
    json!({"status": "ready", "text": text(d, by_handle), "max": crate::viewers::number(d.max, false), "mean": crate::viewers::number(d.mean, false),
           "differing": d.differing, "nan": d.nan, "elements": d.elements, "identical": d.differing == 0, "pictures": pictures})
}

/// The difference in words, as the egui editor's Compare says it.
pub fn text(d: &Diff, by_handle: bool) -> String {
    if d.differing == 0 {
        let how = if by_handle { ": one value" } else { "" };
        return format!("A and B are identical ({} elements){how}", d.elements);
    }
    let mut s = format!(
        "|A−B| max {} · mean {} · {} of {} elements differ",
        crate::viewers::number(d.max, false),
        crate::viewers::number(d.mean, false),
        d.differing,
        d.elements
    );
    if d.nan > 0 {
        s.push_str(&format!(" · {} NaN (in magenta)", d.nan));
    }
    s
}

impl Diffs {
    /// Compare's difference of the two pinned values in its mode (module docs); null unless
    /// Compare shows the difference.
    pub fn difference(&self, session: &Session, media: &MediaService) -> Value {
        let c = &session.views.compare;
        if c.how != CompareHow::Difference {
            return Value::Null;
        }
        let entry = |role| session.run.entry(role).and_then(|(_, d)| d);
        let (a, b) = match (side(entry(WatchRole::PinA)), side(entry(WatchRole::PinB))) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) => return json!({"status": "failed", "error": format!("A: {e}")}),
            (_, Err(e)) => return json!({"status": "failed", "error": format!("B: {e}")}),
        };
        let mode = c.diff;
        let key_part = |r: &DataRef| r.handle.as_ref().map(ToString::to_string).unwrap_or_else(|| format!("{:?}", r.at));
        let key = format!("{}|{}|{}", key_part(&a.0), key_part(&b.0), if mode == DiffMode::Channels { "channels" } else { "mean" });
        if let Some(job) = lock(&self.jobs).get(&key) {
            return match job {
                Job::Working => json!({"status": "pending"}),
                Job::Done(v) => v.clone(),
            };
        }
        let read = |(r, want, names): &(DataRef, Want, Vec<String>), which: &str| -> Result<Option<Pixels>, Value> {
            match media.get(r, *want).bytes() {
                Fetch::Ready(blob) => Pixels::of(&blob, names).map(Some).map_err(|e| json!({"status": "failed", "error": format!("{which}: {e}")})),
                Fetch::Pending => Ok(None),
                Fetch::Missing => Err(json!({"status": "failed", "error": format!("{which}: the value's bytes are gone: run again")})),
                Fetch::Failed(e) => Err(json!({"status": "failed", "error": format!("{which}: {e}")})),
            }
        };
        let (pa, pb) = match (read(&a, "A"), read(&b, "B")) {
            (Ok(Some(pa)), Ok(Some(pb))) => (pa, pb),
            (Err(e), _) | (_, Err(e)) => return e,
            _ => return json!({"status": "pending"}),
        };
        // equal handles are equal bytes: identical, with no need to look
        if a.0.handle.is_some() && a.0.handle == b.0.handle {
            let v = match diff::identical(pa.grid, &pa.names, mode, diff::PICTURE_PX) {
                Ok(d) => json_of(&d, true, &key, media),
                Err(e) => json!({"status": "failed", "error": e}),
            };
            self.keep(key, Job::Done(v.clone()));
            return v;
        }
        self.keep(key.clone(), Job::Working);
        let jobs = Arc::clone(&self.jobs);
        let media = media.clone();
        let spawned = std::thread::Builder::new().name("plumcraft-compare".into()).spawn(move || {
            let v = match diff::diff(&pa, &pb, mode, diff::PICTURE_PX, NAN_COLOUR, &|| false) {
                Ok(d) => json_of(&d, false, &key, &media),
                Err(e) => json!({"status": "failed", "error": e}),
            };
            lock(&jobs).insert(key, Job::Done(v));
        });
        if spawned.is_err() {
            return json!({"status": "failed", "error": "no thread to compute the difference on"});
        }
        json!({"status": "pending"})
    }

    fn keep(&self, key: String, job: Job) {
        let mut jobs = lock(&self.jobs);
        if jobs.len() >= MAX_KEPT && !jobs.contains_key(&key) {
            // the oldest isn't known: start again (a few small JSON values)
            jobs.retain(|_, j| matches!(j, Job::Working));
        }
        jobs.insert(key, job);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_store::Blob;

    #[test]
    fn the_text_says_identical_or_how_far_apart() {
        let blob =
            |v: &[f32]| Arc::new(Blob { dtype: "<f4".into(), shape: vec![2, 2], bytes: v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>().into() });
        let a = Pixels::of(&blob(&[0.0, 1.0, 2.0, 3.0]), &[]).unwrap();
        let b = Pixels::of(&blob(&[0.0, 1.0, 2.5, f32::NAN]), &[]).unwrap();
        let d = diff::diff(&a, &b, DiffMode::Mean, diff::PICTURE_PX, NAN_COLOUR, &|| false).unwrap();
        assert_eq!((d.differing, d.nan), (2, 1));
        assert!(text(&d, false).starts_with("|A−B| max 0.5 · mean 0.166667 · 2 of 4 elements differ · 1 NaN"), "{}", text(&d, false));
        let same = diff::identical(a.grid, &[], DiffMode::Mean, diff::PICTURE_PX).unwrap();
        assert_eq!(text(&same, true), "A and B are identical (4 elements): one value");
        // its pictures are kept by the media service, lent by their handles
        let m = MediaService::default();
        let v = json_of(&d, false, "k", &m);
        let h: Handle = v["pictures"][0]["handle"].as_str().unwrap().parse().unwrap();
        let r = DataRef { handle: Some(h), meta: Default::default(), at: Vec::new() };
        assert!(matches!(m.get(&r, Want::Rendition), Fetch::Ready(Made::Picture(_))), "lent as any picture");
    }
}
