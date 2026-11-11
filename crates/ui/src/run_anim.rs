//! Run animation: each node's body coloured by its state while a run computes the graph, and
//! "slow motion", a minimum time each node is shown running, so a fast graph can be watched.
//!
//! The engines aren't slowed: this is a replay of what the session recorded
//! ([`plumcraft_session::run::RunState::log`]), paced for the eye. A node is shown running no
//! sooner than it really started, than the node before it on the same engine was shown done
//! (an engine computes one node at a time), and than every input it waits for was shown done.
//! So the order is the run's own, and engines that worked side by side are shown side by side.
//! Once the run, and its replay, are over, the colours hold for [`HOLD`], fade over [`FADE`],
//! and the canvas is at rest again.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use plumcraft_proto::event::NodeState;

/// How long the colours stay after the last node is shown finished.
pub const HOLD: Duration = Duration::from_millis(1000);
/// How long they take to fade after that.
pub const FADE: Duration = Duration::from_millis(600);
/// The slow-motion times offered (View › Slow Motion), in milliseconds; 0 is off.
pub const SLOW_CHOICES: &[u32] = &[0, 250, 500, 1000];
/// The longest slow-motion time taken.
pub const MAX_SLOW_MS: u32 = 5000;

/// A node's state as the canvas shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    Waiting,
    Running,
    Done,
    Cached,
    Failed,
    /// Skipped or cancelled.
    Skipped,
}

impl Shown {
    pub fn name(self) -> &'static str {
        match self {
            Shown::Waiting => "waiting",
            Shown::Running => "running",
            Shown::Done => "done",
            Shown::Cached => "cached",
            Shown::Failed => "failed",
            Shown::Skipped => "skipped",
        }
    }

    /// The finished state a run's terminal record shows; `None` for one that isn't.
    fn finished(state: NodeState) -> Option<Shown> {
        match state {
            NodeState::Done => Some(Shown::Done),
            NodeState::Cached => Some(Shown::Cached),
            NodeState::Error => Some(Shown::Failed),
            NodeState::Skipped | NodeState::Cancelled => Some(Shown::Skipped),
            _ => None,
        }
    }
}

/// Where a node sits in the run: what it waits for, and the engine (lane) that computes it.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// Each node's upstream nodes (along wires).
    pub upstream: BTreeMap<String, Vec<String>>,
    /// Each node's engine; nodes on one engine are shown one at a time.
    pub lane: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
struct Track {
    /// When it is shown starting.
    start: Option<Instant>,
    /// When it is shown finished, and as what.
    end: Option<(Instant, Shown)>,
}

/// One run's replay.
#[derive(Debug, Clone, Default)]
pub struct RunAnim {
    generation: u64,
    slow: Duration,
    plan: Plan,
    nodes: BTreeMap<String, Track>,
    lane_free: BTreeMap<String, Instant>,
    /// Records of the session's log taken so far.
    read: usize,
    /// When the real run ended (its reply, a failure or a cancel).
    ended: Option<Instant>,
    /// When the replay began.
    begun: Option<Instant>,
}

impl RunAnim {
    /// The generation of the run being replayed (0: none yet).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// A new run: `nodes` are the ones it computes, all waiting.
    pub fn begin(&mut self, generation: u64, nodes: &BTreeSet<String>, plan: Plan, slow: Duration) {
        *self = RunAnim {
            generation,
            slow,
            plan,
            nodes: nodes.iter().map(|n| (n.clone(), Track::default())).collect(),
            begun: Some(Instant::now()),
            ..RunAnim::default()
        };
    }

    /// Take the records of `log` not yet taken (the session's, of this run, in arrival order).
    pub fn feed(&mut self, log: &[(String, NodeState, Instant)]) {
        for (node, state, at) in log.iter().skip(self.read) {
            self.take(node, *state, *at);
        }
        self.read = self.read.max(log.len());
    }

    /// The real run is over (`at`): a node it never finished is no longer shown.
    pub fn end(&mut self, at: Instant) {
        self.ended.get_or_insert(at);
    }

    fn take(&mut self, node: &str, state: NodeState, at: Instant) {
        let Some(track) = self.nodes.get(node) else { return };
        if track.end.is_some() {
            return;
        }
        let finished = Shown::finished(state);
        let running = matches!(state, NodeState::Started | NodeState::Progress | NodeState::Preview);
        if !running && finished.is_none() {
            return;
        }
        let start = match track.start {
            Some(s) => s,
            None => {
                let s = self.earliest_start(node, at);
                if let Some(t) = self.nodes.get_mut(node) {
                    t.start = Some(s);
                }
                s
            }
        };
        let Some(shown) = finished else { return };
        let least = match shown {
            Shown::Done | Shown::Failed => self.slow,
            Shown::Cached => self.slow / 4,
            _ => Duration::ZERO,
        };
        let end = at.max(start + least);
        if let Some(t) = self.nodes.get_mut(node) {
            t.end = Some((end, shown));
        }
        if let Some(lane) = self.plan.lane.get(node) {
            let free = self.lane_free.entry(lane.clone()).or_insert(end);
            *free = (*free).max(end);
        }
    }

    /// When `node`, really starting at `at`, can be shown starting.
    fn earliest_start(&self, node: &str, at: Instant) -> Instant {
        let mut s = at;
        if let Some(free) = self.plan.lane.get(node).and_then(|l| self.lane_free.get(l)) {
            s = s.max(*free);
        }
        for up in self.plan.upstream.get(node).into_iter().flatten() {
            if let Some((end, _)) = self.nodes.get(up).and_then(|t| t.end) {
                s = s.max(end);
            }
        }
        s
    }

    /// `node` as shown at `now`; `None` when it isn't part of the run being shown.
    pub fn shown(&self, node: &str, now: Instant) -> Option<Shown> {
        let t = self.nodes.get(node)?;
        match (t.start, t.end) {
            (_, Some((end, shown))) if now >= end => Some(shown),
            // a run that ended without finishing it (a failure, a cancel): nothing to show
            (_, None) if self.ended.is_some() => None,
            (Some(start), _) if now >= start => Some(Shown::Running),
            _ => Some(Shown::Waiting),
        }
    }

    /// When the replay is over: the real run has ended and every node is shown finished.
    fn finished_at(&self) -> Option<Instant> {
        let ended = self.ended?;
        Some(self.nodes.values().filter_map(|t| t.end.map(|(e, _)| e)).fold(ended, Instant::max))
    }

    /// How strongly the colours show at `now`: 1 during the run and its replay and for
    /// [`HOLD`] after, then down to 0 over [`FADE`]. 0 with no run.
    pub fn strength(&self, now: Instant) -> f32 {
        if self.generation == 0 {
            return 0.0;
        }
        let Some(done) = self.finished_at() else { return 1.0 };
        let since = now.saturating_duration_since(done);
        if since <= HOLD { 1.0 } else { (1.0 - (since - HOLD).as_secs_f32() / FADE.as_secs_f32()).clamp(0.0, 1.0) }
    }

    /// Where a running node's pulse is, 0 to 1, a beat every 1.2 s from the run's start.
    pub fn phase(&self, now: Instant) -> f32 {
        let since = self.begun.map_or(Duration::ZERO, |b| now.saturating_duration_since(b));
        (since.as_secs_f32() / 1.2).fract()
    }

    /// Is the replay still running or fading (the canvas has to keep drawing)?
    pub fn is_active(&self, now: Instant) -> bool {
        self.strength(now) > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// a → b → c on one engine; d on another, beside them.
    fn plan() -> Plan {
        Plan {
            upstream: [("b", vec!["a"]), ("c", vec!["b"])].into_iter().map(|(k, v)| (k.to_owned(), v.into_iter().map(str::to_owned).collect())).collect(),
            lane: [("a", "e1"), ("b", "e1"), ("c", "e1"), ("d", "e2")].into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(),
        }
    }

    fn nodes() -> BTreeSet<String> {
        ["a", "b", "c", "d"].map(str::to_owned).into()
    }

    fn rec(node: &str, state: NodeState, at: Instant) -> (String, NodeState, Instant) {
        (node.to_owned(), state, at)
    }

    #[test]
    fn without_slow_motion_it_shows_the_run_as_it_happened() {
        let t0 = Instant::now();
        let mut a = RunAnim::default();
        a.begin(1, &nodes(), plan(), Duration::ZERO);
        assert_eq!(a.shown("a", t0), Some(Shown::Waiting));
        assert_eq!(a.shown("zz", t0), None, "not in the run");
        a.feed(&[rec("a", NodeState::Started, t0 + ms(1))]);
        assert_eq!(a.shown("a", t0 + ms(2)), Some(Shown::Running));
        a.feed(&[rec("a", NodeState::Started, t0 + ms(1)), rec("a", NodeState::Done, t0 + ms(5))]);
        assert_eq!(a.shown("a", t0 + ms(5)), Some(Shown::Done));
        assert_eq!(a.shown("b", t0 + ms(5)), Some(Shown::Waiting));
    }

    #[test]
    fn slow_motion_keeps_each_node_running_a_while_in_order() {
        let t0 = Instant::now();
        let mut a = RunAnim::default();
        a.begin(1, &nodes(), plan(), ms(500));
        // the whole run took 3 ms
        a.feed(&[
            rec("a", NodeState::Started, t0),
            rec("a", NodeState::Done, t0 + ms(1)),
            rec("b", NodeState::Cached, t0 + ms(2)),
            rec("c", NodeState::Started, t0 + ms(2)),
            rec("c", NodeState::Done, t0 + ms(3)),
        ]);
        a.end(t0 + ms(3));
        assert_eq!(a.shown("a", t0 + ms(400)), Some(Shown::Running), "a runs its 500 ms");
        assert_eq!(a.shown("b", t0 + ms(400)), Some(Shown::Waiting), "b waits for a");
        assert_eq!(a.shown("b", t0 + ms(550)), Some(Shown::Running), "a cache hit shows for a quarter");
        assert_eq!(a.shown("b", t0 + ms(630)), Some(Shown::Cached));
        assert_eq!(a.shown("c", t0 + ms(630)), Some(Shown::Running), "then c");
        assert_eq!(a.shown("c", t0 + ms(1130)), Some(Shown::Done));
        assert_eq!(a.strength(t0 + ms(1125) + HOLD), 1.0, "held");
        assert!(a.strength(t0 + ms(1130) + HOLD + FADE / 2) < 1.0, "fading");
        assert!(!a.is_active(t0 + ms(1130) + HOLD + FADE + ms(1)), "at rest");
    }

    #[test]
    fn another_engine_works_alongside() {
        let t0 = Instant::now();
        let mut a = RunAnim::default();
        a.begin(1, &nodes(), plan(), ms(500));
        a.feed(&[rec("a", NodeState::Done, t0 + ms(1)), rec("d", NodeState::Done, t0 + ms(2))]);
        assert_eq!(a.shown("d", t0 + ms(100)), Some(Shown::Running), "d's engine isn't waiting for a's");
        assert_eq!(a.shown("a", t0 + ms(100)), Some(Shown::Running));
    }

    #[test]
    fn a_skipped_node_waits_for_what_it_depends_on_then_shows_at_once() {
        let t0 = Instant::now();
        let mut a = RunAnim::default();
        a.begin(1, &nodes(), plan(), ms(500));
        a.feed(&[rec("a", NodeState::Error, t0 + ms(1)), rec("b", NodeState::Skipped, t0 + ms(1)), rec("c", NodeState::Skipped, t0 + ms(1))]);
        assert_eq!(a.shown("b", t0 + ms(300)), Some(Shown::Waiting));
        assert_eq!(a.shown("a", t0 + ms(501)), Some(Shown::Failed));
        assert_eq!(a.shown("c", t0 + ms(501)), Some(Shown::Skipped));
    }

    #[test]
    fn a_run_that_ends_early_stops_showing_what_it_never_reached() {
        let t0 = Instant::now();
        let mut a = RunAnim::default();
        a.begin(1, &nodes(), plan(), Duration::ZERO);
        a.feed(&[rec("a", NodeState::Started, t0)]);
        a.end(t0 + ms(10));
        assert_eq!(a.shown("b", t0 + ms(11)), None);
        assert_eq!(a.shown("a", t0 + ms(11)), None, "it never finished");
    }

    #[test]
    fn no_run_no_colour() {
        assert_eq!(RunAnim::default().strength(Instant::now()), 0.0);
    }
}
