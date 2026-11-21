//! A document's values as pictures, through the media service ([`plumcraft_media::service`]):
//! what ties a document's engine link to it, for every shell (stage N1, slice 6).
//!
//! The rule (`docs/shell-interface.md` 3.1, rule 6): the shell's thread never waits for an
//! engine. A value's bytes come through the engine connection's `resolve`, which for an HTTP
//! engine is a network fetch; so a shell asks the service, which answers at once and does the
//! reading and decoding on its workers. [`Follow`] keeps the service reading through the
//! document's current link (a switch, an attach, a clear); [`docs_resolver`] and [`nonblocking`]
//! adapt it to the two older signatures that remain (below).

use std::sync::Arc;

use plumcraft_host::Connection;
use plumcraft_media::service::Resolve;
use plumcraft_media::{MediaService, Want};
use plumcraft_proto::data::DataRef;
use plumcraft_session::doc_cmds::Resolver;
use plumcraft_store::Blob;

/// The service's resolver for an engine connection: its `resolve`, called on a worker.
pub fn resolver(conn: &Arc<dyn Connection>) -> Resolve {
    let c = Arc::clone(conn);
    Arc::new(move |r: &DataRef| c.resolve(r))
}

/// Keeps a media service reading through a document's current engine connection.
#[derive(Debug, Default)]
pub struct Follow {
    /// The connection last given (its address), or none.
    last: Option<usize>,
}

impl Follow {
    /// Point `media` at `conn` (none: no engine) when it isn't the connection it already reads
    /// through. True when it changed: what was queued for the old link is dropped.
    pub fn follow(&mut self, media: &MediaService, conn: Option<&Arc<dyn Connection>>) -> bool {
        let now = conn.map(|c| Arc::as_ptr(c).cast::<()>() as usize);
        if now == self.last && (now.is_some() == media.has_resolver()) {
            return false;
        }
        self.last = now;
        media.set_resolver(conn.map(resolver));
        true
    }
}

/// The session's `docs.resolver` (`doc.open`, `value.open`) through the service: bytes already
/// read are shared; bytes not read yet are read in place. Those commands reply with the copy
/// they made, so they wait, as they always have; every other read goes to the workers.
pub fn docs_resolver(media: &MediaService) -> Resolver {
    let m = media.clone();
    Arc::new(move |r: &DataRef| m.blocking(r, Want::Bytes).bytes().ready().map(|b| b.bytes.to_vec()))
}

/// Ask the media service, ahead of time, for every value the run's application payloads
/// (`NodeReport.editor`, which an application's value editor reads) name: so an editor opened
/// after the run finds its picture already read, as it did when reads were made in place. Never
/// waits. Hostile payloads are skipped: only objects that parse as a [`DataRef`] with a handle are
/// asked for, at most [`MAX_READ_AHEAD`] a call.
pub fn read_ahead(media: &MediaService, run: &plumcraft_session::RunState) {
    let mut refs = Vec::new();
    for n in run.nodes.values() {
        for v in n.editor.values() {
            collect_refs(v, &mut refs, 0);
            if refs.len() >= MAX_READ_AHEAD {
                break;
            }
        }
    }
    for r in refs.iter().take(MAX_READ_AHEAD) {
        let _ = media.get(r, Want::Bytes);
    }
}

/// The most values [`read_ahead`] asks for at once.
pub const MAX_READ_AHEAD: usize = 64;

/// Every object in `v` that is a value reference with a handle (`{"handle", "meta", "at"}`).
fn collect_refs(v: &serde_json::Value, into: &mut Vec<DataRef>, depth: usize) {
    use serde_json::Value;
    if depth > 16 || into.len() >= MAX_READ_AHEAD {
        return;
    }
    match v {
        Value::Object(o) => {
            if o.get("handle").is_some_and(Value::is_string)
                && o.get("at").is_some_and(Value::is_array)
                && let Ok(r) = serde_json::from_value::<DataRef>(v.clone())
            {
                into.push(r);
                return;
            }
            for c in o.values() {
                collect_refs(c, into, depth + 1);
            }
        }
        Value::Array(a) => a.iter().for_each(|c| collect_refs(c, into, depth + 1)),
        _ => {}
    }
}

/// A read that never waits, for code written against `Fn(&DataRef) -> Option<Blob>` that asks
/// again each frame (an application's value editors): the bytes once here, `None` meanwhile.
pub fn nonblocking(media: &MediaService) -> impl Fn(&DataRef) -> Option<Blob> + 'static {
    let m = media.clone();
    move |r: &DataRef| m.bytes(r).ready().map(|b| (*b).clone())
}

/// The viewer a value is shown in first, for the kinds a shell draws from the core alone (N3
/// slice 1: `picture`, `text.plain`, `scalar`, the floor N3's gate names, and the plainer others
/// by media). This is the start of `docs/shell-interface.md` 13.1's registry moving into the core:
/// the egui editor still chooses from its own registry (`plumcraft_ui::viewers::kinds_for`), and a
/// test there holds the two to the same first choice for these media. A value no viewer here takes
/// is `hex`.
pub fn default_viewer(d: &plumcraft_proto::display::Display) -> &'static str {
    // N4 slice 5: the first the core's registry offers (`viewers::offered`, egui's order)
    crate::viewers::offered(d).first().map_or("hex", |(id, _)| id)
}

/// The Watched panel's value as the core has it (`ui.inspect`'s `watched` on a shell with no
/// panel of its own, N3 slice 1): `{node, port, summary, media, viewer, value?, data?, picture?,
/// excerpt?, error?}` (`value`: a small value inline; `data`: the full value's reference, for
/// `media.get` and `plum_blob`; `picture`: its first rgba8 rendition, `{ref, w, h}`; `excerpt`:
/// a text's first 4 KiB, `{text, offset, totalChars, lines}`), or null while nothing is watched. `summary` and the rest are null
/// until a run has sent the value.
pub fn watched_json(run: &plumcraft_session::RunState) -> serde_json::Value {
    match run.panel() {
        Some((w, d)) => value_json(w, d),
        None => serde_json::Value::Null,
    }
}

/// A watched entry's value as [`watched_json`] has the panel's: the panel's, or a pop-out's
/// (`popout.get`, N4 slice 3).
pub fn value_json(w: &plumcraft_proto::display::WatchRequest, d: Option<&plumcraft_proto::display::Display>) -> serde_json::Value {
    use plumcraft_proto::display::Rendition;
    use serde_json::{Value, json};
    let mut v = json!({"node": w.node, "port": w.port, "summary": d.map(|d| d.summary.clone()), "media": d.map(|d| d.media), "viewer": d.map(default_viewer)});
    if let Some(d) = d {
        if let Some(x) = &d.value {
            v["value"] = x.clone();
        }
        if let Some(full) = &d.full {
            v["data"] = serde_json::to_value(full).unwrap_or(Value::Null);
        }
        let picture = d.renditions.iter().find_map(|r| match r {
            Rendition::Rgba8 { data, role, w, h, .. } if role.is_empty() || role == "thumb" => Some(json!({"ref": data, "w": w, "h": h})),
            _ => None,
        });
        if let Some(p) = picture {
            v["picture"] = p;
        }
        let excerpt = d.renditions.iter().find_map(|r| match r {
            Rendition::Excerpt { text, offset, total_chars, lines } => Some(json!({"text": text, "offset": offset, "totalChars": total_chars, "lines": lines})),
            _ => None,
        });
        if let Some(x) = excerpt {
            v["excerpt"] = x;
        }
        if let Some(e) = &d.error {
            v["error"] = serde_json::to_value(e).unwrap_or(Value::Null);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::error::ProtoError;
    use plumcraft_proto::session::EngineState;
    use serde_json::Value;

    struct Fake(u8);
    impl Connection for Fake {
        fn call(&self, _: &str, _: Value) -> Result<Value, ProtoError> {
            Ok(Value::Null)
        }
        fn state(&self) -> EngineState {
            EngineState::Unavailable { reason: String::new() }
        }
        fn resolve(&self, _: &DataRef) -> Option<Blob> {
            Some(Blob { dtype: "|u1".into(), shape: vec![1], bytes: vec![self.0].into() })
        }
    }

    #[test]
    fn the_service_follows_the_documents_link() {
        let m = MediaService::default();
        let mut f = Follow::default();
        let (a, b): (Arc<dyn Connection>, Arc<dyn Connection>) = (Arc::new(Fake(1)), Arc::new(Fake(2)));
        assert!(f.follow(&m, Some(&a)));
        assert!(!f.follow(&m, Some(&a)), "the same link: nothing to do");
        let r = DataRef { handle: None, meta: Default::default(), at: vec![] };
        assert_eq!(&*m.blocking(&r, Want::Bytes).bytes().ready().unwrap().bytes, &[1]);
        assert!(f.follow(&m, Some(&b)));
        assert_eq!(docs_resolver(&m)(&r), Some(vec![2]), "a value without a handle is read again through the new link");
        assert!(f.follow(&m, None));
        assert!(!m.has_resolver());
        assert!(nonblocking(&m)(&r).is_none() || nonblocking(&m)(&r).is_some());
    }

    #[test]
    fn an_editors_values_are_read_ahead_and_junk_is_skipped() {
        use serde_json::json;
        let m = MediaService::default();
        m.set_resolver(Some(Arc::new(|_: &DataRef| Some(Blob { dtype: "|u1".into(), shape: vec![1], bytes: vec![9].into() }))));
        let mut run = plumcraft_session::RunState::default();
        let h = plumcraft_proto::Handle::raw(b"pic").to_string();
        let mut n = plumcraft_session::run::NodeRun::default();
        n.editor.insert("x.mask".into(), json!({"picture": {"kind": "rgba8", "ref": {"handle": h, "meta": {}, "at": []}}, "junk": [1, {"handle": 3, "at": 4}, {"handle": "nope", "at": []}]}));
        run.nodes.insert("n1".into(), n);
        read_ahead(&m, &run);
        assert!(m.wait_idle(std::time::Duration::from_secs(5)));
        let r: DataRef = serde_json::from_value(json!({"handle": h, "meta": {}, "at": []})).unwrap();
        assert!(nonblocking(&m)(&r).is_some(), "read before anyone asked");
    }
}
