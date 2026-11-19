//! Run states and results as words: node headers, the status bar, tooltips. Moved from
//! `plumcraft-ui`'s `run_view` and `canvas` to the core (stage N1), so every shell says the same.

use plumcraft_proto::event::NodeState;
use plumcraft_session::run::NodeRun;

/// Characters of an error shown on a node; the rest is on hover.
pub const ERROR_CHARS: usize = 180;
/// Lines and characters of a text excerpt shown on a node.
pub const EXCERPT_LINES: usize = 3;
pub const EXCERPT_CHARS: usize = 200;

/// A node's header text when no run has completed it yet (and none is computing it).
pub const NOT_RUN: &str = "not run";
/// Its hover.
pub const NOT_RUN_HOVER: &str = "No run has completed this node yet. Run to compute it.";

/// A run state as a word (the node header, the status bar, tooltips).
pub fn state_name(s: NodeState) -> &'static str {
    match s {
        NodeState::Queued => "queued",
        NodeState::Started | NodeState::Progress | NodeState::Preview => "running",
        NodeState::Done => "done",
        NodeState::Cached => "cached",
        NodeState::Error => "error",
        NodeState::Skipped => "skipped",
        NodeState::Cancelled => "cancelled",
        NodeState::Unknown => "",
    }
}

/// Does the node show as stale (its label and border)? Only a completed node's result can be
/// out of date (B21), and while a run is computing the node again its run state shows instead:
/// queued or running is what matters then (B20).
pub fn shows_stale(run: &NodeRun) -> bool {
    run.is_stale() && run.completed && !run.is_active()
}

/// A running node's header text: "running", with how far it has got when the node says.
pub fn running_text(fraction: Option<f64>) -> String {
    match fraction.filter(|f| f.is_finite()) {
        Some(f) => format!("running {:.0}%", (f * 100.0).clamp(0.0, 100.0)),
        None => "running".to_owned(),
    }
}

/// Is the node computing now?
pub fn is_running(s: NodeState) -> bool {
    matches!(s, NodeState::Started | NodeState::Progress | NodeState::Preview)
}

/// A cached node's header text and its hover: what computing it took, and what fetching did.
pub fn cached_text(computed: Option<f64>, fetched: Option<f64>) -> (String, String) {
    match (computed, fetched) {
        (Some(c), Some(f)) => (format!("cached · {}", format_ms(c)), format!("Computed in {}, fetched in {}", format_ms(c), format_ms(f))),
        (Some(c), None) => (format!("cached · {}", format_ms(c)), format!("Computed in {}", format_ms(c))),
        (None, Some(f)) => ("cached".to_owned(), format!("Fetched in {}", format_ms(f))),
        (None, None) => ("cached".to_owned(), "From the cache".to_owned()),
    }
}

/// A duration for a node header or the status bar: `0.42 ms`, `12 ms`, `1.24 s`.
pub fn format_ms(ms: f64) -> String {
    if !ms.is_finite() || ms < 0.0 {
        "—".into()
    } else if ms < 10.0 {
        format!("{ms:.2} ms")
    } else if ms < 1000.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

/// At most `n` characters of `s`, with an ellipsis when cut. True when cut.
pub fn truncate_chars(s: &str, n: usize) -> (String, bool) {
    match s.char_indices().nth(n) {
        Some((i, _)) => (format!("{}…", s.get(..i).unwrap_or(s).trim_end()), true),
        None => (s.to_owned(), false),
    }
}

/// A text excerpt as a node shows it: its first [`EXCERPT_LINES`] lines and at most
/// [`EXCERPT_CHARS`] characters, so one long line can't stretch the node.
pub fn excerpt_lines(text: &str) -> String {
    let short: String = text.lines().take(EXCERPT_LINES).collect::<Vec<_>>().join("\n");
    let (short, cut) = truncate_chars(&short, EXCERPT_CHARS);
    if !cut && text.lines().nth(EXCERPT_LINES).is_some() { format!("{short}\n…") } else { short }
}

/// The hover of a thumbnail from an earlier run.
pub const OLDER_THUMB: &str = "From the previous run: this run hasn't reached the node yet.";

/// Why a node is stale: the edits since its last run that changed it or what feeds it.
pub fn stale_tooltip(run: &NodeRun) -> String {
    if run.stale_why.is_empty() {
        return "Stale: an edit since the last run changed this node or something upstream.".into();
    }
    let lines: Vec<String> = run.stale_why.iter().map(|w| format!("• {w}")).collect();
    format!("Stale since the last run:\n{}\nRun again to bring it up to date.", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(format_ms(0.213), "0.21 ms");
        assert_eq!(format_ms(41.6), "42 ms");
        assert_eq!(format_ms(1234.0), "1.23 s");
        assert_eq!(format_ms(f64::NAN), "—");
        assert_eq!(cached_text(Some(4410.0), Some(0.01)), ("cached · 4.41 s".to_owned(), "Computed in 4.41 s, fetched in 0.01 ms".to_owned()));
        assert_eq!(cached_text(None, None).0, "cached");
    }

    #[test]
    fn long_errors_and_excerpts_are_cut() {
        assert_eq!(truncate_chars("short", 10), ("short".into(), false));
        let (t, cut) = truncate_chars("ééééééé", 3);
        assert!(cut && t == "ééé…");
        assert_eq!(excerpt_lines("a\nb"), "a\nb");
        assert_eq!(excerpt_lines("a\nb\nc\nd"), "a\nb\nc\n…");
        assert_eq!(excerpt_lines(&"x".repeat(500)).chars().count(), EXCERPT_CHARS + 1);
    }

    #[test]
    fn states_have_words() {
        assert_eq!(state_name(NodeState::Progress), "running");
        assert!(is_running(NodeState::Preview) && !is_running(NodeState::Done));
    }
}
