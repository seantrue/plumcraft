//! A memo hit whose file is gone (spec 4.6: a reaped value is a miss, never an error). A file
//! under the engine's work root is put back from the commonkit store when the store holds it,
//! else the node runs again; a file still there is touched so the work root's reaper keeps it;
//! a file anywhere else is only checked.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use plumcraft_engine::data::Plane;
use plumcraft_engine::exec::{self, Memo};
use plumcraft_engine::{Blob, BlobSource, CancelToken, Cx, Data, Emit, Engine, Inputs, Node, Outputs};
use plumcraft_proto::Handle;
use plumcraft_proto::catalogue::{Media, NodeDef, Port, WireType};
use plumcraft_proto::data::{BlobMeta, Form};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::event::{NodeState, StatusRecord};
use plumcraft_proto::run::{RunReply, RunRequest};
use plumcraft_proto::session::EngineInfo;
use plumcraft_store::Store;
use serde_json::json;

/// Writes `text` to `<dir>/out.txt` and returns it as a file blob; counts its runs.
struct WriteFile {
    dir: PathBuf,
    runs: Arc<AtomicUsize>,
}

impl Node for WriteFile {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "t.file".into(),
            inputs: vec![Port { name: "text".into(), ty: "STRING".into(), default: json!("hello"), ..Default::default() }],
            outputs: vec![Port { name: "file".into(), ty: "TXT".into(), ..Default::default() }],
            ..Default::default()
        }
    }

    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let text = i.get("text").and_then(Data::as_str).unwrap_or_default().to_owned();
        let path = self.dir.join("out.txt");
        std::fs::create_dir_all(&self.dir).map_err(|e| NodeError::new(NodeError::OP, e.to_string()))?;
        std::fs::write(&path, &text).map_err(|e| NodeError::new(NodeError::OP, e.to_string()))?;
        let meta = BlobMeta { media: "text/plain".into(), size: text.len() as u64, form: Form::Doc { app: "t".into() }, aliases: Vec::new() };
        let blob = Blob { handle: Handle::md5_raw(text.as_bytes()), meta, source: BlobSource::File(path), publish: true, thumb: None };
        Ok(Outputs::from([("file".to_owned(), Data::Blob(Box::new(blob)))]))
    }
}

#[derive(Default)]
struct Log(Vec<StatusRecord>);

impl Emit for Log {
    fn status(&mut self, rec: StatusRecord) {
        self.0.push(rec);
    }
}

fn tmp(name: &str) -> PathBuf {
    let d = plumcraft_scratch::root().join(format!("memo-files-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// An engine whose work root is `work`, its node writing into `out`.
fn engine(work: &Path, out: &Path) -> (Engine, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let info = EngineInfo { id: "t".into(), salt: "t".into(), ..Default::default() };
    let e = Engine::new(info)
        .with_store(None)
        .with_work_root(work.to_path_buf())
        .with_wire_type(WireType { id: "TXT".into(), media: Media::Doc, ..Default::default() })
        .with_node(Box::new(WriteFile { dir: out.to_path_buf(), runs: runs.clone() }));
    (e, runs)
}

/// One run of `n` (target), the value published to `store` when there is one.
fn run(e: &Engine, memo: &mut Memo, store: Option<&Store>) -> RunReply {
    let req: RunRequest = serde_json::from_value(json!({
        "run_id": "r", "generation": 1,
        "graph": {"kind": "inline", "graph": {"version": 1, "nodes": {"n": {"kind": "t.file", "values": {"text": "hello, store"}}}, "wires": []}},
        "targets": ["n"],
    }))
    .unwrap();
    let plane = Plane { store, shm: true, urls: false, max_inline: 0, algo: plumcraft_proto::handle::Algo::Md5 };
    exec::run(e, &req, &mut Log::default(), &CancelToken::default(), "test", memo, &plane)
}

#[test]
fn a_reaped_work_file_is_put_back_from_the_store() {
    let dir = tmp("restore");
    let store = Store::new(dir.join("store"));
    let (e, runs) = engine(&dir, &dir.join("work"));
    let mut memo = Memo::default();
    assert_eq!(run(&e, &mut memo, Some(&store)).nodes["n"].state, NodeState::Done);
    let file = dir.join("work/out.txt");
    std::fs::remove_file(&file).unwrap();
    let again = run(&e, &mut memo, Some(&store));
    assert_eq!(again.nodes["n"].state, NodeState::Cached, "{again:?}");
    assert_eq!(std::fs::read(&file).unwrap(), b"hello, store", "put back whole");
    assert_eq!(runs.load(Ordering::SeqCst), 1, "not run again");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_reaped_work_file_not_in_the_store_runs_the_node_again() {
    let dir = tmp("rerun");
    let (e, runs) = engine(&dir, &dir.join("work"));
    let mut memo = Memo::default();
    run(&e, &mut memo, None);
    std::fs::remove_file(dir.join("work/out.txt")).unwrap();
    let again = run(&e, &mut memo, None);
    assert_eq!(again.nodes["n"].state, NodeState::Done, "a miss, not an error: {again:?}");
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    // A file that changed size is a miss too.
    std::fs::write(dir.join("work/out.txt"), b"short").unwrap();
    assert_eq!(run(&e, &mut memo, None).nodes["n"].state, NodeState::Done);
    assert_eq!(runs.load(Ordering::SeqCst), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_hit_touches_a_work_file_and_never_a_file_outside_it() {
    let dir = tmp("touch");
    let (e, _) = engine(&dir.join("work-root"), &dir.join("work-root/out"));
    let mut memo = Memo::default();
    run(&e, &mut memo, None);
    let file = dir.join("work-root/out/out.txt");
    let old = SystemTime::now() - Duration::from_secs(86_400);
    std::fs::File::options().append(true).open(&file).unwrap().set_modified(old).unwrap();
    assert_eq!(run(&e, &mut memo, None).nodes["n"].state, NodeState::Cached);
    assert!(std::fs::metadata(&file).unwrap().modified().unwrap() > old + Duration::from_secs(3600), "touched");

    // Outside the work root (a user's file): checked, never touched.
    let (e, runs) = engine(&dir.join("work-root"), &dir.join("elsewhere"));
    let mut memo = Memo::default();
    run(&e, &mut memo, None);
    let file = dir.join("elsewhere/out.txt");
    std::fs::File::options().append(true).open(&file).unwrap().set_modified(old).unwrap();
    assert_eq!(run(&e, &mut memo, None).nodes["n"].state, NodeState::Cached);
    assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), old, "not touched");
    std::fs::remove_file(&file).unwrap();
    assert_eq!(run(&e, &mut memo, None).nodes["n"].state, NodeState::Done, "gone: run again");
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    let _ = std::fs::remove_dir_all(&dir);
}
