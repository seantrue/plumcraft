//! A workflow bundle is hostile input (golden rule 1, spec 18), and joins the sweep of
//! `crates/proto/tests/hostile.rs` here, where the reader lives: truncations and every flipped
//! byte of a real bundle, entries that leave their folder, links, bombs and lying sizes. Each is a
//! clean error or a clean unpack, never a panic, and never a byte outside the new folder.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_session::Session;
use plumcraft_session::archive::{Limits, extract_bundle, extract_within};
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;

const WORKFLOW: &[u8] = br#"{"version": 1, "nodes": {}, "wires": []}"#;

/// A folder of this test's own.
fn folder() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let d = plumcraft_scratch::dir("hostile-bundles").join(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (n, b) in entries {
        z.start_file(*n, SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).unwrap();
        z.write_all(b).unwrap();
    }
    z.finish().unwrap().into_inner()
}

/// Every file and folder under `dir`.
fn listing(dir: &Path) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                todo.push(p.clone());
            }
            out.insert(p);
        }
    }
    out
}

/// Refused, with nothing left: `root` holds just the zip.
fn refused(root: &Path, name: &str, bytes: &[u8], limits: Limits, says: &str) {
    let p = root.join(name);
    std::fs::write(&p, bytes).unwrap();
    let before = listing(root);
    let e = extract_within(&p, limits).expect_err(name).to_string();
    assert!(e.contains(says), "{name}: {e}");
    assert_eq!(listing(root), before, "{name}: nothing written");
}

/// Find `sig` records, the central directory's file headers.
fn central_headers(bytes: &[u8]) -> Vec<usize> {
    bytes.windows(4).enumerate().filter(|(_, w)| *w == b"PK\x01\x02").map(|(i, _)| i).collect()
}

fn set_u32(bytes: &mut [u8], at: usize, v: u32) {
    bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn entries_that_leave_their_folder_are_refused() {
    let root = folder();
    for (i, name) in ["../evil.txt", "a/../../evil.txt", "/tmp/plumcraft-hostile-evil.txt", "a/../../../evil.txt", "C:\\evil.txt"].iter().enumerate() {
        refused(&root, &format!("slip{i}.zip"), &zip_of(&[("w.json", WORKFLOW), (name, b"x")]), Limits::default(), "leaves its folder");
    }
    assert!(!root.parent().unwrap().join("evil.txt").exists());
    assert!(!Path::new("/tmp/plumcraft-hostile-evil.txt").exists());
}

#[test]
fn a_backslash_name_stays_inside() {
    let root = folder();
    let p = root.join("back.zip");
    std::fs::write(&p, zip_of(&[("w.json", WORKFLOW), ("..\\evil.txt", b"x")])).unwrap();
    let outcome = extract_bundle(&p);
    assert!(!root.parent().unwrap().join("evil.txt").exists());
    if let Ok(wf) = outcome {
        assert!(wf.starts_with(root.join("back")));
    }
}

#[test]
fn a_link_is_refused() {
    let root = folder();
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    z.start_file("w.json", SimpleFileOptions::default()).unwrap();
    z.write_all(WORKFLOW).unwrap();
    z.add_symlink("up", "/etc", SimpleFileOptions::default()).unwrap();
    z.start_file("up/evil.txt", SimpleFileOptions::default()).unwrap();
    z.write_all(b"x").unwrap();
    refused(&root, "link.zip", &z.finish().unwrap().into_inner(), Limits::default(), "is a link");
}

#[test]
fn two_entries_of_one_name_never_overwrite() {
    let root = folder();
    // made as a.txt and b.txt, then the name patched: a writer won't make this
    let mut bytes = zip_of(&[("w.json", WORKFLOW), ("a.txt", b"first"), ("b.txt", b"second")]);
    for i in 0..bytes.len() - 5 {
        if &bytes[i..i + 5] == b"b.txt" {
            bytes[i] = b'a';
        }
    }
    let p = root.join("twice.zip");
    std::fs::write(&p, bytes).unwrap();
    // the reader keeps one of them; either way one file stands, nothing is overwritten by surprise
    if let Ok(wf) = extract_bundle(&p) {
        let dest = wf.parent().unwrap();
        assert_eq!(std::fs::read_dir(dest).unwrap().count(), 2, "w.json and one a.txt");
    }
}

#[test]
fn a_file_where_a_folder_must_go_is_a_clean_error() {
    let root = folder();
    refused(&root, "clash.zip", &zip_of(&[("w.json", WORKFLOW), ("x", b"file"), ("x/y", b"under it")]), Limits::default(), "can't unpack");
}

#[test]
fn no_workflow_is_refused() {
    let root = folder();
    refused(&root, "none.zip", &zip_of(&[("a/w.json", WORKFLOW)]), Limits::default(), "found 0");
    refused(&root, "empty.zip", &zip_of(&[]), Limits::default(), "found 0");
    refused(&root, "two.zip", &zip_of(&[("note.txt", b"x"), ("a.json", WORKFLOW), ("b.json", WORKFLOW)]), Limits::default(), "found 2");
}

#[test]
fn too_many_entries_and_too_deep_are_refused() {
    let root = folder();
    let many: Vec<(String, &[u8])> = (0..11).map(|i| (format!("{i}.txt"), &b"x"[..])).collect();
    let many: Vec<(&str, &[u8])> = many.iter().map(|(n, b)| (n.as_str(), *b)).collect();
    refused(&root, "many.zip", &zip_of(&many), Limits { entries: 10, ..Limits::default() }, "more than 10 entries");
    let deep = format!("{}f.txt", "d/".repeat(70));
    refused(&root, "deep.zip", &zip_of(&[("w.json", WORKFLOW), (&deep, b"x")]), Limits::default(), "folders deep");
}

/// A real bomb: a megabyte of nothing per entry, and a bound lower than it comes to.
#[test]
fn a_bomb_is_stopped_before_it_is_written() {
    let root = folder();
    let nothing = vec![0u8; 1 << 20];
    let bytes = zip_of(&[("w.json", WORKFLOW), ("a.bin", &nothing), ("b.bin", &nothing)]);
    assert!(bytes.len() < 1 << 14, "it compresses: {}", bytes.len());
    refused(&root, "bomb.zip", &bytes, Limits { unpacked: 1 << 20, ..Limits::default() }, "unpacks to more than");
}

/// Past the real bound, by what the entries declare: 3 GiB and 3 GiB of a tiny file. Nothing is
/// inflated to find out.
#[test]
fn declared_sizes_past_the_bound_are_refused_at_once() {
    let root = folder();
    let mut bytes = zip_of(&[("w.json", WORKFLOW), ("a.bin", b"x"), ("b.bin", b"y")]);
    for at in central_headers(&bytes) {
        set_u32(&mut bytes, at + 24, 3 << 30);
    }
    refused(&root, "declared.zip", &bytes, Limits::default(), "unpacks to more than");
}

/// An entry that declares a byte and inflates to a megabyte is stopped by what really comes out,
/// or by its checksum, and leaves nothing.
#[test]
fn an_entry_that_lies_about_its_size_leaves_nothing() {
    let root = folder();
    let mut bytes = zip_of(&[("w.json", WORKFLOW), ("a.bin", &vec![0u8; 1 << 20])]);
    for at in central_headers(&bytes) {
        set_u32(&mut bytes, at + 24, 1);
    }
    refused(&root, "liar.zip", &bytes, Limits { unpacked: 1 << 16, ..Limits::default() }, "can't unpack");
}

#[test]
fn every_truncation_and_flipped_byte_is_safe() {
    let root = folder();
    let good = zip_of(&[("w.json", WORKFLOW), ("files/a.txt", b"hello hello hello"), ("sub/b.txt", b"world")]);
    let p = root.join("sweep.zip");
    let check = |bytes: &[u8], what: &str| {
        std::fs::write(&p, bytes).unwrap();
        if let Ok(wf) = extract_bundle(&p) {
            assert!(wf.starts_with(&root), "{what}: {}", wf.display());
        }
        for e in std::fs::read_dir(&root).unwrap() {
            let path = e.unwrap().path();
            assert!(path.starts_with(&root));
            if path != p {
                assert!(path.is_dir(), "{what}: {}", path.display());
                std::fs::remove_dir_all(&path).unwrap();
            }
        }
    };
    for n in 0..good.len() {
        check(&good[..n], &format!("the first {n} bytes"));
    }
    for at in 0..good.len() {
        for flip in [0x01, 0x80, 0xFF] {
            let mut b = good.clone();
            b[at] ^= flip;
            check(&b, &format!("byte {at} ^ {flip:#x}"));
        }
    }
    for junk in [&b""[..], b"PK", b"PK\x05\x06", b"not a zip at all", &[0xFF; 4096]] {
        check(junk, "junk");
    }
}

fn session() -> Session {
    Session::new(SchemaReply::default())
}

#[test]
fn opening_a_hostile_bundle_changes_nothing() {
    let root = folder();
    let mut s = session();
    let before = s.graph().clone();
    for (name, bytes) in [
        ("slip-open.zip", zip_of(&[("w.json", WORKFLOW), ("../evil.txt", b"x")])),
        ("junk-open.zip", b"not a zip".to_vec()),
        ("notjson.zip", zip_of(&[("w.json", b"[1, 2")])),
        ("wrongjson.zip", zip_of(&[("w.json", br#"{"version": 99, "nodes": "many"}"#)])),
    ] {
        let p = root.join(name);
        std::fs::write(&p, bytes).unwrap();
        let listed = listing(&root);
        for id in ["graph.open", "graph.insert"] {
            let r = s.execute(id, &json!({"path": p}));
            assert!(r.is_err(), "{id} {name}: {r:?}");
            assert_eq!(s.graph(), &before, "{id} {name}");
            assert_eq!(s.path(), None);
            assert_eq!(listing(&root), listed, "{id} {name}: a refused bundle leaves no folder");
        }
    }
}

#[test]
fn a_bundle_opens_through_the_command() {
    let root = folder();
    let p = root.join("fine.zip");
    std::fs::write(&p, zip_of(&[("w.json", WORKFLOW)])).unwrap();
    let mut s = session();
    let r = s.execute("graph.open", &json!({"path": p})).unwrap();
    assert_eq!(r["unpackedFrom"], Value::String(p.display().to_string()));
    assert_eq!(s.path(), Some(root.join("fine/w.json").as_path()), "saves go in the new folder");
}
