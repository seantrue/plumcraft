//! The engine worker: every call a document makes to its engines, made off the thread that owns
//! the document (`docs/shell-interface.md` 3.1 rule 6: the UI thread never waits for an engine).
//!
//! One thread per document, started when it is first needed, takes jobs in the order they were
//! given: so a cancel goes out after the run it cancels, a release after the runs of its graph,
//! and an old engine is told and stopped ([`Retired`]) before its successor starts. What comes
//! back ([`Done`]) waits in an inbox until the owner drains it on its own thread (the egui shell
//! at the start of a frame; the FFI's core thread when woken), and the owner's [`Waker`], the same
//! one its [`crate::link::Link`] wakes, says something is there. The session is never touched
//! here: the owner applies what comes back (`RunState::fail`, [`Switchboard::finish`]).
//!
//! Most jobs need no answer: a run that the engine took says so in its events, as before. A run it
//! refused comes back as [`Done::RunFailed`], which the owner applies only if that run is still
//! the latest (a superseded run's failure is old news, as its late events are). An engine switch's
//! waiting part ([`Work`]) comes back as [`Done::Switched`].
//!
//! **Shutdown.** Dropping the worker stops it: jobs not yet started are dropped unrun (dropping
//! them drops what they hold, links included, on the worker's thread), the thread ends after the
//! job in hand, and the drop waits for that at most [`SHUTDOWN_WAIT`]. A job still inside an
//! engine call then (a hung engine) is left to finish on its own; every host's calls time out,
//! after which the thread ends. Nothing ever joins a thread that might not end.
//!
//! [`Switchboard::finish`]: crate::switchboard::Switchboard::finish

use std::any::Any;
use std::panic::AssertUnwindSafe;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use plumcraft_host::Connection;
use serde_json::{Value, json};

use crate::link::{Link, Waker};
use crate::switchboard::{Retired, Work, Worked};

/// How long dropping the worker waits for the job in hand.
pub const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// What a job hands back to the document's owner.
pub enum Done {
    /// The engine refused a run, or it couldn't be sent: the run fails with `why`, if it is still
    /// the latest (`run_id` and `generation` are the session's latest).
    RunFailed { run_id: String, generation: u64, why: String },
    /// An engine switch's waiting part, for [`crate::switchboard::Switchboard::finish`], and
    /// what the owner gave the job to carry (its connect function), given back.
    Switched { worked: Box<Worked>, back: Box<dyn Any + Send> },
}

impl std::fmt::Debug for Done {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Done::RunFailed { run_id, generation, why } => write!(f, "RunFailed({run_id}, {generation}, {why})"),
            Done::Switched { .. } => f.write_str("Switched"),
        }
    }
}

type Job = Box<dyn FnOnce() -> Option<Done> + Send>;

#[derive(Default)]
struct State {
    inbox: Vec<Done>,
    /// Jobs given and not yet finished.
    queued: usize,
    /// The owner has gone: drop what is left unrun.
    stop: bool,
    /// The thread has ended.
    exited: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    waker: Mutex<Option<Waker>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Shared {
    fn finished(&self, done: Option<Done>) {
        let woke = done.is_some();
        {
            let mut s = lock(&self.state);
            s.inbox.extend(done);
            s.queued = s.queued.saturating_sub(1);
        }
        self.changed.notify_all();
        if woke && let Some(w) = lock(&self.waker).clone() {
            w.wake();
        }
    }
}

/// A document's engine worker (see the module docs).
#[derive(Default)]
pub struct EngineWorker {
    tx: Option<Sender<Job>>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for EngineWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = lock(&self.shared.state);
        write!(f, "EngineWorker {{ queued: {}, waiting: {} }}", s.queued, s.inbox.len())
    }
}

impl EngineWorker {
    /// A worker that wakes `waker` when something comes back. Its thread starts with the first job.
    pub fn new(waker: Option<Waker>) -> EngineWorker {
        let w = EngineWorker::default();
        w.set_waker(waker);
        w
    }

    /// Wake this from now on.
    pub fn set_waker(&self, waker: Option<Waker>) {
        *lock(&self.shared.waker) = waker;
    }

    pub fn has_waker(&self) -> bool {
        lock(&self.shared.waker).is_some()
    }

    fn submit(&mut self, job: Job) {
        lock(&self.shared.state).queued += 1;
        if self.tx.is_none() {
            let (tx, rx) = channel::<Job>();
            let shared = Arc::clone(&self.shared);
            let started = std::thread::Builder::new().name("plumcraft-engine-worker".into()).spawn(move || {
                for job in rx {
                    let stopping = lock(&shared.state).stop;
                    // an engine that panics must not stop the worker (or leave its count up)
                    let done = if stopping { None } else { std::panic::catch_unwind(AssertUnwindSafe(job)).unwrap_or(None) };
                    shared.finished(done);
                }
                lock(&shared.state).exited = true;
                shared.changed.notify_all();
            });
            match started {
                Ok(t) => {
                    self.tx = Some(tx);
                    self.thread = Some(t);
                }
                // no thread to be had: the job runs here rather than not at all
                Err(_) => return self.shared.finished(job()),
            }
        }
        if let Some(tx) = &self.tx
            && let Err(e) = tx.send(job)
        {
            // the thread is gone (it can't be, short of a panic in the loop itself): run it here
            self.shared.finished((e.0)());
        }
    }

    /// Send a run (`body`, a serialized `RunRequest`) to `conn`. A refusal comes back as
    /// [`Done::RunFailed`].
    pub fn run(&mut self, conn: Arc<dyn Connection>, body: Value, run_id: String, generation: u64) {
        self.submit(Box::new(move || conn.call("run", body).err().map(|e| Done::RunFailed { run_id, generation, why: e.message })));
    }

    /// Call `method` on `conn` and let the answer go (`graph.release`, `run.cancel`: an engine
    /// without the capability has nothing to release, and a cancelled run's late records are
    /// ignored by generation whatever the reply).
    pub fn call(&mut self, conn: Arc<dyn Connection>, method: &'static str, params: Value) {
        self.submit(Box::new(move || {
            let _ = conn.call(method, params);
            None
        }));
    }

    /// `run.cancel` to `conn`.
    pub fn cancel(&mut self, conn: Arc<dyn Connection>, run_id: String) {
        self.call(conn, "run.cancel", json!({"run_id": run_id}));
    }

    /// Tell an old engine its runs are cancelled and stop it, here rather than on the owner's
    /// thread (dropping a link joins its pump; dropping a spawned engine's last holder stops it).
    pub fn retire(&mut self, retired: Vec<Retired>) {
        if retired.iter().all(Retired::is_empty) {
            return;
        }
        self.submit(Box::new(move || {
            retired.into_iter().for_each(Retired::stop);
            None
        }));
    }

    /// Do an engine switch's waiting part ([`Work::run`]), starting engines with `connect` over
    /// `carry` (the owner's connect function, which comes back in [`Done::Switched`]).
    pub fn switch<C: Send + 'static>(&mut self, work: Work, carry: C, mut connect: impl FnMut(&mut C, &str, &str) -> Result<Link, String> + Send + 'static) {
        self.submit(Box::new(move || {
            let mut carry = carry;
            let worked = work.run(&mut |engine: &str, mode: &str| connect(&mut carry, engine, mode));
            Some(Done::Switched { worked: Box::new(worked), back: Box::new(carry) })
        }));
    }

    /// What came back since the last drain.
    pub fn drain(&self) -> Vec<Done> {
        std::mem::take(&mut lock(&self.shared.state).inbox)
    }

    /// Nothing given is unfinished and nothing waits to be drained.
    pub fn idle(&self) -> bool {
        let s = lock(&self.shared.state);
        s.queued == 0 && s.inbox.is_empty()
    }

    /// Jobs given and not yet finished.
    pub fn queued(&self) -> usize {
        lock(&self.shared.state).queued
    }

    /// Wait until something can be drained or nothing is left to do, at most `timeout`: whether
    /// it came to that. For callers that may wait (tests, the synchronous API), never a frame.
    pub fn wait(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut s = lock(&self.shared.state);
        while s.inbox.is_empty() && s.queued > 0 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            s = self.shared.changed.wait_timeout(s, left).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
        true
    }
}

impl Drop for EngineWorker {
    fn drop(&mut self) {
        lock(&self.shared.state).stop = true;
        // closing the queue ends the thread's loop once the job in hand is done
        drop(self.tx.take());
        let Some(thread) = self.thread.take() else { return };
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        let mut s = lock(&self.shared.state);
        while !s.exited {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                // a hung engine call: the thread ends when it returns; nothing waits for it
                return;
            }
            s = self.shared.changed.wait_timeout(s, left).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
        drop(s);
        let _ = thread.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use plumcraft_proto::data::DataRef;
    use plumcraft_proto::error::{ErrorCode, ProtoError};
    use plumcraft_proto::session::EngineState;
    use plumcraft_store::Blob;

    /// An engine that notes each call (method, thread) and refuses runs of `bad`.
    #[derive(Default)]
    struct Log(Mutex<Vec<(String, std::thread::ThreadId)>>, Duration);

    impl Connection for Log {
        fn call(&self, method: &str, params: Value) -> Result<Value, ProtoError> {
            std::thread::sleep(self.1);
            let what = format!("{method} {}", params.get("run_id").and_then(Value::as_str).unwrap_or_default());
            self.0.lock().unwrap().push((what, std::thread::current().id()));
            if params["run_id"] == "bad" { Err(ProtoError::new(ErrorCode::BadParams, "no")) } else { Ok(json!({})) }
        }
        fn state(&self) -> EngineState {
            EngineState::Off
        }
        fn resolve(&self, _: &DataRef) -> Option<Blob> {
            None
        }
    }

    #[test]
    fn jobs_run_in_order_off_the_callers_thread_and_wake_it_with_what_came_back() {
        let woken = Arc::new(AtomicUsize::new(0));
        let w2 = Arc::clone(&woken);
        let mut w = EngineWorker::new(Some(Waker::new(move || {
            w2.fetch_add(1, Ordering::SeqCst);
        })));
        assert!(w.idle() && w.thread.is_none(), "no thread until there is work");
        let log = Arc::new(Log(Mutex::default(), Duration::from_millis(30)));
        let conn: Arc<dyn Connection> = log.clone();
        let t = Instant::now();
        w.run(Arc::clone(&conn), json!({"run_id": "r1"}), "r1".into(), 1);
        w.cancel(Arc::clone(&conn), "r1".into());
        w.run(Arc::clone(&conn), json!({"run_id": "bad"}), "bad".into(), 2);
        w.call(Arc::clone(&conn), "graph.release", json!({"run_id": "scope"}));
        assert!(t.elapsed() < Duration::from_millis(30), "the caller doesn't wait: {:?}", t.elapsed());
        assert!(!w.idle());
        let deadline = Instant::now() + Duration::from_secs(10);
        while w.queued() > 0 {
            assert!(w.wait(Duration::from_secs(10)) && Instant::now() < deadline);
            if w.queued() > 0 {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        let calls = log.0.lock().unwrap().clone();
        assert_eq!(calls.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(), ["run r1", "run.cancel r1", "run bad", "graph.release scope"], "in order");
        assert!(calls.iter().all(|(_, t)| *t != std::thread::current().id()), "none on this thread");
        let done = w.drain();
        assert!(matches!(done.as_slice(), [Done::RunFailed { run_id, generation: 2, why }] if run_id == "bad" && why == "no"), "{done:?}");
        assert_eq!(woken.load(Ordering::SeqCst), 1, "woken for what came back, not for every job");
        assert!(w.idle());
    }

    #[test]
    fn dropping_it_stops_its_thread_and_never_waits_long_for_a_hung_engine() {
        // a quick engine: the drop joins the thread
        let mut w = EngineWorker::new(None);
        let log = Arc::new(Log::default());
        w.cancel(log.clone(), "a".into());
        assert!(w.wait(Duration::from_secs(10)));
        let t = Instant::now();
        drop(w);
        assert!(t.elapsed() < Duration::from_secs(1));

        // jobs not started are dropped unrun; one stuck in a call is left to end on its own
        let mut w = EngineWorker::new(None);
        let slow = Arc::new(Log(Mutex::default(), Duration::from_secs(3)));
        for i in 0..5 {
            w.cancel(slow.clone(), format!("{i}"));
        }
        std::thread::sleep(Duration::from_millis(50));
        let t = Instant::now();
        drop(w);
        let took = t.elapsed();
        assert!(took >= SHUTDOWN_WAIT && took < SHUTDOWN_WAIT + Duration::from_secs(1), "{took:?}");
        // the one in hand finishes, and nothing after it runs
        std::thread::sleep(Duration::from_millis(1500));
        assert_eq!(slow.0.lock().unwrap().len(), 1);
    }
}
