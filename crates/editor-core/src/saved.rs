//! A remote save's files, written here (spec 13.2 step 8, 15): an engine that can't write where
//! the person chose (no `data.path`: its host left `path` out of the request) returns each file
//! as a blob, `files: [{name, ref}]`, and the editor fetches and writes them. One file is the
//! path chosen; several (a list) go beside it, numbered from its name (`<stem>-00.<ext>`, …) as
//! an engine numbers a list it writes itself. The engine's names are never used as paths, so a
//! reply can't write outside the folder the person chose. Each is written whole (a file beside
//! it, then a rename), so nothing ever reads half of one.

use std::path::{Path, PathBuf};

use plumcraft_host::Connection;
use plumcraft_proto::error::NodeError;
use plumcraft_proto::run::{SaveReply, SaveRequest};

/// `reply` with the files it returned written and named in `paths`; a reply that wrote its own
/// (or failed) as it came. A file that can't be fetched or written fails the save.
pub fn write_returned(conn: &dyn Connection, req: &SaveRequest, mut reply: SaveReply) -> SaveReply {
    if !reply.ok || reply.files.is_empty() || !reply.paths.is_empty() {
        return reply;
    }
    let Some(dest) = req.path.as_deref().map(PathBuf::from) else {
        return SaveReply::failed(NodeError::engine("the engine returned files, and no path was chosen for them"));
    };
    let several = reply.files.len() > 1;
    let dir = dest.parent().map(Path::to_path_buf).unwrap_or_default();
    let (stem, ext) = stem_ext(&dest);
    // numbered as an engine numbers a list it writes itself: two digits, more when needed
    let width = (reply.files.len() - 1).to_string().len().max(2);
    let mut written = Vec::new();
    for (i, f) in reply.files.iter().enumerate() {
        let target = if several { dir.join(format!("{stem}-{i:0width$}{ext}")) } else { dest.clone() };
        let Some(blob) = f.data.as_ref().and_then(|r| conn.resolve(r)) else {
            return SaveReply::failed(NodeError::engine(format!("the engine's file {} couldn't be fetched", target.display())));
        };
        if let Err(e) = write_whole(&target, &blob.bytes) {
            return SaveReply::failed(NodeError::new(NodeError::OP, format!("{}: {e}", target.display())));
        }
        written.push(target.display().to_string());
    }
    reply.paths = written;
    reply
}

fn stem_ext(p: &Path) -> (String, String) {
    let stem = p.file_stem().map_or_else(|| "result".to_owned(), |s| s.to_string_lossy().into_owned());
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (stem, ext)
}

/// Write `bytes` to `p` whole: a file beside it, then a rename.
fn write_whole(p: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = p.parent().filter(|d| !d.as_os_str().is_empty()).map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!(".{name}.{}.saving", std::process::id()));
    std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, p)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use plumcraft_proto::data::{BlobMeta, DataRef};
    use plumcraft_proto::run::SavedFile;
    use plumcraft_proto::session::EngineState;
    use plumcraft_proto::{Handle, ProtoError};
    use serde_json::Value;

    use super::*;

    struct Inline;
    impl Connection for Inline {
        fn call(&self, _: &str, _: Value) -> Result<Value, ProtoError> {
            Ok(Value::Null)
        }
        fn state(&self) -> EngineState {
            EngineState::Connecting { url: String::new() }
        }
        fn resolve(&self, r: &DataRef) -> Option<plumcraft_store::Blob> {
            plumcraft_store::read_inline(r)
        }
    }

    fn file(name: &str, bytes: &[u8]) -> SavedFile {
        let meta = BlobMeta { size: bytes.len() as u64, ..Default::default() };
        SavedFile { name: name.into(), data: Some(plumcraft_store::inline_ref(Handle::raw(bytes), meta, bytes)) }
    }

    #[test]
    fn returned_files_are_written_where_the_person_chose_and_nowhere_else() {
        let dir = plumcraft_scratch::dir("saved-returned");
        let req = SaveRequest { path: Some(dir.join("one.txt").display().to_string()), ..Default::default() };
        let r = write_returned(&Inline, &req, SaveReply { ok: true, files: vec![file("result.txt", b"plum")], ..Default::default() });
        assert!(r.ok && r.paths == [dir.join("one.txt").display().to_string()], "{r:?}");
        assert_eq!(std::fs::read(dir.join("one.txt")).unwrap(), b"plum", "one file is the path chosen, whatever the engine named it");
        let req = SaveRequest { path: Some(dir.join("many.txt").display().to_string()), ..Default::default() };
        let r =
            write_returned(&Inline, &req, SaveReply { ok: true, files: vec![file("a-00.txt", b"a"), file("../../escape.txt", b"b")], ..Default::default() });
        assert!(r.ok, "{r:?}");
        assert_eq!(r.paths, [dir.join("many-00.txt").display().to_string(), dir.join("many-01.txt").display().to_string()]);
        assert_eq!(std::fs::read(dir.join("many-00.txt")).unwrap(), b"a", "numbered from the name chosen, as a local save is");
        assert_eq!(std::fs::read(dir.join("many-01.txt")).unwrap(), b"b");
        assert!(!dir.join("a-00.txt").exists() && !dir.join("../../escape.txt").exists(), "the engine's names are never paths");
        // a file that can't be fetched fails the save; a reply that wrote its own is as it came
        let lost = SaveReply { ok: true, files: vec![SavedFile { name: "x".into(), data: None }], ..Default::default() };
        assert!(!write_returned(&Inline, &req, lost).ok);
        let own = SaveReply { ok: true, paths: vec!["/elsewhere".into()], ..Default::default() };
        assert_eq!(write_returned(&Inline, &req, own.clone()), own);
        let nowhere = SaveRequest::default();
        assert!(!write_returned(&Inline, &nowhere, SaveReply { ok: true, files: vec![file("a", b"a")], ..Default::default() }).ok);
    }
}
