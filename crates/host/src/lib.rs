//! The UI side of the interface: one [`Connection`] whatever the binding, so the editor (and the
//! conformance suite) never knows which kind of engine it has.
//!
//! - [`InProc`]: mode 1, the engine compiled in ([`plumcraft_engine::host::Host`]).
//! - [`process::ProcessHost`]: mode 2, a separate process on a Unix socket, supervised.
//! - [`http::HttpHost`]: mode 3, an engine over HTTP, here or on another machine.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod http;
pub mod manifest;
pub mod process;
pub mod ring;

use plumcraft_engine::host::Host;
use plumcraft_proto::data::DataRef;
use plumcraft_proto::error::ProtoError;
use plumcraft_proto::session::EngineState;
use plumcraft_store::{Blob, Store};
use serde_json::Value;

/// Every operation of spec 6.1, by name, with JSON params and result.
pub trait Connection: Send + Sync {
    fn call(&self, method: &str, params: Value) -> Result<Value, ProtoError>;
    /// What the host knows about the engine (spec 12.4).
    fn state(&self) -> EngineState;
    /// A blob's bytes, by the first locator this session may use (spec 4.4). A miss is `None`,
    /// never an error (spec 4.6).
    fn resolve(&self, r: &DataRef) -> Option<Blob>;
    /// The folders this engine's `path` locators lie in, when another engine on this machine can
    /// read them: its store and work root (spec 7.6 rule 10). None for a remote engine.
    fn readable_roots(&self) -> Vec<String> {
        Vec::new()
    }
    /// The local files a `run` or `output.save` with `params` would send to an engine nobody here
    /// started, not yet allowed: `(where to, file)`. A window asks before it sends them.
    fn uploads_pending(&self, _params: &Value) -> Vec<(String, String)> {
        Vec::new()
    }
    /// Let these files go to the engine, for as long as this connection lasts.
    fn allow_uploads(&self, _files: &[String]) {}
}

impl<T: Connection + ?Sized> Connection for Box<T> {
    fn call(&self, method: &str, params: Value) -> Result<Value, ProtoError> {
        (**self).call(method, params)
    }

    fn state(&self) -> EngineState {
        (**self).state()
    }

    fn resolve(&self, r: &DataRef) -> Option<Blob> {
        (**self).resolve(r)
    }

    fn readable_roots(&self) -> Vec<String> {
        (**self).readable_roots()
    }

    fn uploads_pending(&self, params: &Value) -> Vec<(String, String)> {
        (**self).uploads_pending(params)
    }

    fn allow_uploads(&self, files: &[String]) {
        (**self).allow_uploads(files);
    }
}

/// Mode 1.
pub struct InProc(pub Host);

impl Connection for InProc {
    fn call(&self, method: &str, params: Value) -> Result<Value, ProtoError> {
        self.0.dispatch(method, params)
    }

    fn readable_roots(&self) -> Vec<String> {
        let roots = [self.0.store_root(), self.0.work_root()];
        roots.into_iter().flatten().map(|r| r.display().to_string()).collect()
    }

    fn state(&self) -> EngineState {
        self.0.state()
    }

    /// Same process: the engine's store root is trusted without a proof.
    fn resolve(&self, r: &DataRef) -> Option<Blob> {
        let root = self.0.store_root()?;
        Store::new(root).resolve(r, true)
    }
}
