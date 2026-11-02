//! The status reducer (spec 9.5, normative): what a UI shows for each node of a run, from status
//! records delivered by any binding (pushed, long-polled or polled; lost, duplicated or
//! reordered) plus the authoritative reply.
//!
//! 1. State is keyed by `(run_id, node_id)`. Records of an older generation are ignored; a newer
//!    generation resets.
//! 2. Within a node, one publisher's records are ordered by its `seq`; different publishers' by
//!    state rank (`queued` < `started` < `progress` < terminal), never by timestamp. A node never
//!    leaves a terminal state.
//! 3. `preview` adds its thumb, outputs and displays whatever the order, without changing a
//!    terminal state.
//! 4. The reply is authoritative: it overwrites every node and fills nodes without a record.
//! 5. Duplicates (the same `(publisher, seq)`) are dropped.

use std::collections::{BTreeMap, BTreeSet};

use crate::display::{Display, Rendition};
use crate::error::NodeError;
use crate::event::{NodeState, StatusRecord};
use crate::run::RunReply;

/// What the UI knows about one node.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodeView {
    pub state: NodeState,
    pub fraction: Option<f64>,
    pub stage: Option<String>,
    pub ms: Option<f64>,
    pub error: Option<NodeError>,
    pub thumb: Option<Rendition>,
    pub displays: Vec<Display>,
}

/// One run's node states.
#[derive(Debug, Clone, Default)]
pub struct Reducer {
    run_id: String,
    generation: u64,
    nodes: BTreeMap<String, NodeView>,
    /// The last seq applied per (node, publisher).
    last: BTreeMap<(String, String), u64>,
    seen: BTreeSet<(String, u64)>,
    reply: Option<RunReply>,
}

impl Reducer {
    pub fn new(run_id: impl Into<String>, generation: u64) -> Self {
        Reducer { run_id: run_id.into(), generation, ..Default::default() }
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn nodes(&self) -> &BTreeMap<String, NodeView> {
        &self.nodes
    }

    pub fn node(&self, id: &str) -> Option<&NodeView> {
        self.nodes.get(id)
    }

    pub fn reply(&self) -> Option<&RunReply> {
        self.reply.as_ref()
    }

    /// Start over for a newer generation of the same UI's runs (rule 1).
    fn reset(&mut self, run_id: &str, generation: u64) {
        *self = Reducer::new(run_id, generation);
    }

    /// Apply one record. Returns whether anything changed.
    pub fn apply(&mut self, run_id: &str, generation: u64, rec: &StatusRecord) -> bool {
        if generation < self.generation || (generation == self.generation && run_id != self.run_id) {
            return false;
        }
        if generation > self.generation {
            self.reset(run_id, generation);
        }
        if rec.state == NodeState::Unknown || rec.node_id.is_empty() {
            return false;
        }
        // Rule 5: a redelivered record.
        if !self.seen.insert((rec.publisher.clone(), rec.seq)) && !rec.publisher.is_empty() {
            return false;
        }
        if self.reply.is_some() {
            return false; // rule 4: the reply already settled everything
        }
        let key = (rec.node_id.clone(), rec.publisher.clone());
        let in_order = self.last.get(&key).is_none_or(|&s| rec.seq > s);
        let node = self.nodes.entry(rec.node_id.clone()).or_default();
        // Rule 3: preview data lands whatever the order.
        if rec.state == NodeState::Preview {
            if rec.thumb.is_some() {
                node.thumb = rec.thumb.clone();
            }
            if !rec.display.is_empty() {
                node.displays = rec.display.clone();
            }
            if node.state.is_terminal() {
                return true;
            }
        }
        // Rule 2: never leave a terminal state; across publishers, order by rank; within one,
        // by seq.
        if node.state.is_terminal() {
            return false;
        }
        let advances = if in_order { rec.state.rank() >= node.state.rank() || node.state == NodeState::Unknown } else { rec.state.rank() > node.state.rank() };
        if !advances {
            return false;
        }
        self.last.insert(key, rec.seq);
        node.state = rec.state;
        if rec.fraction.is_some() {
            node.fraction = rec.fraction;
        }
        if rec.stage.is_some() {
            node.stage = rec.stage.clone();
        }
        if rec.ms.is_some() {
            node.ms = rec.ms;
        }
        if rec.error.is_some() {
            node.error = rec.error.clone();
        }
        true
    }

    /// Apply the authoritative reply (rule 4). A reply of another run or generation is ignored.
    pub fn finish(&mut self, reply: &RunReply) -> bool {
        if reply.generation < self.generation || (reply.generation == self.generation && reply.run_id != self.run_id) {
            return false;
        }
        if reply.generation > self.generation {
            self.reset(&reply.run_id, reply.generation);
        }
        for (id, report) in &reply.nodes {
            let node = self.nodes.entry(id.clone()).or_default();
            node.state = report.state;
            node.ms = Some(report.ms);
            node.error = report.error.clone();
            if report.thumb.is_some() {
                node.thumb = report.thumb.clone();
            }
        }
        // Nodes the reply doesn't mention but records left mid-flight: a cancelled or failed run
        // never reached them.
        if !reply.ok {
            for node in self.nodes.values_mut().filter(|n| !n.state.is_terminal()) {
                node.state = NodeState::Cancelled;
            }
        }
        self.reply = Some(reply.clone());
        true
    }

    /// Every node in a terminal state, and the reply in.
    pub fn is_finished(&self) -> bool {
        self.reply.is_some()
    }

    /// The state of every node, for comparing two deliveries of one run.
    pub fn states(&self) -> BTreeMap<String, NodeState> {
        self.nodes.iter().map(|(k, v)| (k.clone(), v.state)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::NodeReport;

    fn rec(node: &str, publisher: &str, seq: u64, state: NodeState) -> StatusRecord {
        StatusRecord { node_id: node.into(), publisher: publisher.into(), seq, state, ..Default::default() }
    }

    #[test]
    fn ordering_and_terminal_states() {
        let mut r = Reducer::new("r", 1);
        assert!(r.apply("r", 1, &rec("n", "p", 1, NodeState::Started)));
        assert!(r.apply("r", 1, &rec("n", "p", 3, NodeState::Done)));
        // A late progress from the same publisher: older seq, dropped.
        assert!(!r.apply("r", 1, &rec("n", "p", 2, NodeState::Progress)));
        assert_eq!(r.node("n").unwrap().state, NodeState::Done);
        // A redelivery.
        assert!(!r.apply("r", 1, &rec("n", "p", 3, NodeState::Done)));
        // A preview after done still lands its thumb.
        let mut pv = rec("n", "p", 4, NodeState::Preview);
        pv.thumb = Some(Rendition::Excerpt { text: "hi".into(), offset: 0, total_chars: 2, lines: 1 });
        assert!(r.apply("r", 1, &pv));
        assert_eq!(r.node("n").unwrap().state, NodeState::Done);
        assert!(r.node("n").unwrap().thumb.is_some());
    }

    #[test]
    fn publishers_order_by_rank_not_clock() {
        let mut r = Reducer::new("r", 1);
        let mut late = rec("n", "worker-b", 1, NodeState::Started);
        late.ts = 100.0;
        let mut early = rec("n", "worker-a", 1, NodeState::Progress);
        early.ts = 1.0;
        r.apply("r", 1, &early);
        assert!(!r.apply("r", 1, &late), "a lagging worker's started doesn't move progress back");
        assert_eq!(r.node("n").unwrap().state, NodeState::Progress);
    }

    #[test]
    fn generations_and_the_reply() {
        let mut r = Reducer::new("r1", 2);
        assert!(!r.apply("r0", 1, &rec("n", "p", 1, NodeState::Started)), "older generation");
        r.apply("r1", 2, &rec("a", "p", 1, NodeState::Started));
        let mut reply = RunReply { run_id: "r1".into(), generation: 2, ok: true, ..Default::default() };
        reply.nodes.insert("a".into(), NodeReport { state: NodeState::Done, ms: 3.0, ..Default::default() });
        reply.nodes.insert("b".into(), NodeReport { state: NodeState::Cached, ..Default::default() });
        assert!(r.finish(&reply));
        assert_eq!(r.node("a").unwrap().state, NodeState::Done);
        assert_eq!(r.node("b").unwrap().state, NodeState::Cached, "filled from the reply");
        assert!(!r.apply("r1", 2, &rec("a", "p", 9, NodeState::Error)), "nothing moves after the reply");
        // A newer generation resets.
        assert!(r.apply("r2", 3, &rec("x", "p", 1, NodeState::Started)));
        assert!(r.node("a").is_none());
    }
}
