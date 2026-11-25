//! Values as plain data through the C ABI (`docs/shell-interface.md` section 10), on an engine
//! whose every read takes 300 ms (`spike.slowResolve`): a thumbnail of an ARRAY value and a
//! decoded picture of an IMAGE value are asked for with `media.get`, answer `pending` at once,
//! arrive in `plum_poll`'s `media` after a wake, and are then lent by `plum_blob`; neither the
//! request nor `plum_blob` ever waits for the engine.

mod common;

use std::time::{Duration, Instant};

use common::{Client, store_root, take};
use plumcraft_ffi::{PLUM_ERR_ARG, PLUM_ERR_UNREADABLE, PLUM_OK, PLUM_PENDING, plum_blob, plum_leases, plum_poll, plum_release};
use serde_json::{Value, json};

/// Every object in `v` that is an `rgba8` rendition: its data's handle.
fn rgba_handles(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            if o.get("kind").and_then(Value::as_str) == Some("rgba8")
                && let Some(h) = o.get("ref").or_else(|| o.get("data")).and_then(|d| d.get("handle")).and_then(Value::as_str)
            {
                out.push(h.to_owned());
            }
            o.values().for_each(|c| rgba_handles(c, out));
        }
        Value::Array(a) => a.iter().for_each(|c| rgba_handles(c, out)),
        _ => {}
    }
}

/// Poll until the feed's `media` names `key` (keeping replies for the client), at most `max`.
fn until_media(c: &mut Client, key: &str, max: Duration) -> Value {
    let start = Instant::now();
    loop {
        let bytes = take(plum_poll(c.core));
        if !bytes.is_empty() {
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            for r in v["replies"].as_array().unwrap() {
                c.replies.insert(r["id"].as_u64().unwrap(), r.clone());
            }
            if let Some(m) = v.get("media").and_then(Value::as_array).and_then(|m| m.iter().find(|m| m["key"] == key)) {
                return m.clone();
            }
        }
        assert!(start.elapsed() < max, "no media item for {key} in {max:?}");
        c.wait_wake(Duration::from_millis(100));
    }
}

/// `plum_blob(key)`: its status, and its bytes (copied) and token when lent.
fn blob(c: &Client, key: &str) -> (i32, Vec<u8>, u64, Duration) {
    let t = Instant::now();
    // SAFETY: the key is live for the call.
    let b = unsafe { plum_blob(c.core, key.as_ptr(), key.len()) };
    let took = t.elapsed();
    if b.status != PLUM_OK {
        assert!(b.ptr.is_null() && b.token == 0);
        return (b.status, Vec::new(), 0, took);
    }
    // SAFETY: lent until released, by the caller.
    let bytes = unsafe { std::slice::from_raw_parts(b.ptr, b.len) }.to_vec();
    (b.status, bytes, b.token, took)
}

#[test]
fn a_thumbnail_and_a_picture_are_pending_then_lent_and_nothing_waits_for_the_engine() {
    let mut c = Client::open(&json!({"engines": {"default": "array"}, "store_root": store_root("media"), "media": {"cache_mb": 32, "workers": 2}}));
    let leases = plum_leases();
    // a 300 × 200 picture with a bright margin, its rendition (the canvas's thumbnail), and the
    // same as a PNG (an IMAGE value)
    let k = c.run("node.add", json!({"kind": "array.constant", "pos": [0, 0]}))["id"].as_str().unwrap().to_owned();
    for (port, value) in [("width", json!(300)), ("height", json!(200)), ("channels", json!(3)), ("value", json!(0.25))] {
        c.run("node.set", json!({"node": k, "port": port, "value": value}));
    }
    let f = c.run("node.add", json!({"kind": "array.frame", "pos": [260, 0]}))["id"].as_str().unwrap().to_owned();
    c.run("node.set", json!({"node": f, "port": "margin", "value": 20}));
    c.run("node.set", json!({"node": f, "port": "margin_value", "value": 0.9}));
    c.run("wire.connect", json!({"from": {"node": k, "port": "constant"}, "to": {"node": f, "port": "array"}}));
    let e = c.run("node.add", json!({"kind": "array.encode_image", "pos": [520, 0]}))["id"].as_str().unwrap().to_owned();
    c.run("wire.connect", json!({"from": {"node": f, "port": "framed"}, "to": {"node": e, "port": "image"}}));
    c.run("run.watch", json!({"node": e}));
    assert_eq!(c.call("run", json!({}))["ok"], true);
    let done = c.call("spike.waitRun", json!({"timeout_ms": 60_000}));
    assert_eq!(done["result"]["running"], false, "{done}");
    let image = done["result"]["watch"][0]["full"]["handle"].as_str().unwrap_or_else(|| panic!("the PNG's handle: {done}")).to_owned();
    let mut thumbs = Vec::new();
    rgba_handles(&Value::Array(c.events.clone()), &mut thumbs);
    let thumb = thumbs.first().unwrap_or_else(|| panic!("an rgba8 thumbnail among {} events", c.events.len())).clone();

    // every read through the engine now takes 300 ms
    assert_eq!(c.call("spike.slowResolve", json!({"ms": 300}))["ok"], true);

    for (handle, want, px) in [(thumb.as_str(), "rgba", None), (image.as_str(), "image", Some(128))] {
        let t = Instant::now();
        let r = c.call("media.get", json!({"handle": handle, "want": want, "px": px}));
        let replied = t.elapsed();
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["result"]["status"], "pending", "{r}");
        assert!(replied < Duration::from_millis(150), "media.get waited for the engine: {replied:?}");
        let key = r["result"]["key"].as_str().unwrap().to_owned();
        // not here yet: plum_blob says so, at once
        let (status, _, _, took) = blob(&c, &key);
        assert_eq!(status, PLUM_PENDING, "{key}");
        assert!(took < Duration::from_millis(50), "plum_blob waited: {took:?}");
        // the feed says when it is here
        let item = until_media(&mut c, &key, Duration::from_secs(20));
        assert!(t.elapsed() >= Duration::from_millis(250), "it was read through the slow engine: {:?}", t.elapsed());
        assert_eq!(item["status"], "ready", "{item}");
        assert_eq!(item["format"], "rgba8");
        let (w, h) = (item["w"].as_u64().unwrap() as usize, item["h"].as_u64().unwrap() as usize);
        let (status, pixels, token, took) = blob(&c, &key);
        assert_eq!(status, PLUM_OK, "{key}");
        assert!(took < Duration::from_millis(50), "lent from what was made: {took:?}");
        assert_eq!(pixels.len(), w * h * 4);
        // the same pixels the editor decodes from the value's own bytes (lent from the store on
        // this machine, never through the engine)
        let (status, raw, raw_token, took) = blob(&c, handle);
        assert_eq!(status, PLUM_OK);
        assert!(took < Duration::from_millis(250), "the store's bytes, not the engine's read: {took:?}");
        let expect = match want {
            "rgba" => raw.clone(),
            _ => plumcraft_media::decode::decode_picture(&raw, 128).unwrap().pixels,
        };
        assert_eq!(pixels, expect, "{want}: the pixels decoded on a worker are the editor's");
        if want == "image" {
            assert_eq!((w, h), (128, 85), "300 × 200 shrunk to 128 on its long side");
            let centre = ((h / 2) * w + w / 2) * 4;
            assert_eq!(&pixels[centre..centre + 4], &[63, 63, 63, 255], "the middle, at 0.25");
        }
        // asked again: from the cache, at once
        let again = c.call("media.get", json!({"handle": handle, "want": want, "px": px}));
        assert_eq!(again["result"]["status"], "ready", "{again}");
        assert_eq!(plum_release(token), PLUM_OK);
        assert_eq!(plum_release(raw_token), PLUM_OK);
    }

    // hostile requests and keys: errors, never a wait or a panic
    for p in [
        json!({}),
        json!({"handle": 7}),
        json!({"handle": ""}),
        json!({"handle": thumb, "want": "pigeon"}),
        json!({"handle": thumb, "want": "image", "px": 0}),
        json!({"handle": thumb, "want": "image", "px": 1e9}),
        json!({"handle": "x".repeat(10_000)}),
    ] {
        let r = c.call("media.get", p.clone());
        assert_eq!(r["ok"], false, "{p}: {r}");
    }
    for key in ["derive:", "derive:pigeon:md5:raw:00", "derive:image:md5:raw:00@0", "derive:image:@12", "derive:svg:x@99999"] {
        assert_eq!(blob(&c, key).0, PLUM_ERR_ARG, "{key}");
    }
    // a value that isn't a picture: asked for as one, it fails with why, and is unreadable
    let r = c.call("media.get", json!({"handle": image, "want": "rgba"}));
    let key = r["result"]["key"].as_str().unwrap().to_owned();
    let item = until_media(&mut c, &key, Duration::from_secs(20));
    assert_eq!(item["status"], "failed", "{item}");
    assert!(item["reason"].as_str().is_some_and(|r| !r.is_empty()), "{item}");
    assert_eq!(blob(&c, &key).0, PLUM_ERR_UNREADABLE);
    let stats = c.call("media.stats", json!({}));
    assert!(stats["result"]["threads"].as_u64().unwrap() >= 1, "{stats}");
    assert_eq!(plum_leases(), leases, "every lease given back");
    c.close();
}

#[test]
fn a_bad_media_config_is_refused_and_the_header_names_the_new_statuses() {
    for bad in [json!({"media": 3}), json!({"media": {"cache_mb": 0}}), json!({"media": {"workers": 99}})] {
        let text = bad.to_string();
        // SAFETY: live for the call; no wake.
        let core = unsafe { plumcraft_ffi::plum_open(text.as_ptr(), text.len(), None, std::ptr::null_mut()) };
        assert_eq!(core, 0, "{bad}");
        let why = String::from_utf8(take(plumcraft_ffi::plum_last_error())).unwrap();
        assert!(why.contains("media"), "{why}");
    }
    let header = include_str!("../include/plumcraft.h");
    assert!(header.contains(&format!("#define PLUM_PENDING {PLUM_PENDING} ")), "PLUM_PENDING");
    assert!(header.contains(&format!("#define PLUM_ERR_UNREADABLE ({PLUM_ERR_UNREADABLE})")), "PLUM_ERR_UNREADABLE");
}
