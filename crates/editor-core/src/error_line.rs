//! The status bar's last error, with a count of the errors said (B167). A shell shows a banner for
//! each error the core reports, so it can't tell two from one by the text: the same refusal twice
//! in a row is two errors with one text. Every report bumps `seq`; the feed's `document` domain
//! and `ui.inspect` carry it as `errorSeq`, and a shell shows a banner when it moves.

use serde::{Serialize, Serializer};

/// The last error said, and how many have been said in this document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ErrorLine {
    text: Option<String>,
    seq: u64,
    /// The text says the graph's problems (B163): it goes when they do.
    problems: bool,
}

impl ErrorLine {
    /// Say `text`: it becomes the last error, and the count rises (the same text again too).
    pub fn set(&mut self, text: impl Into<String>) {
        self.text = Some(text.into());
        self.seq = self.seq.saturating_add(1);
        self.problems = false;
    }
    /// Say the graph's problems (B163): a line the document takes away once the graph has none
    /// ([`crate::document::Document::recheck_problems`]), however they went (an engine joined, a
    /// node removed), rather than a picture of them that stays.
    pub fn set_problems(&mut self, text: impl Into<String>) {
        self.set(text);
        self.problems = true;
    }
    /// Whether the line says the graph's problems.
    pub fn about_problems(&self) -> bool {
        self.problems && self.text.is_some()
    }
    /// Say `text` if there is one, else clear the line.
    pub fn put(&mut self, text: Option<String>) {
        match text {
            Some(t) => self.set(t),
            None => self.clear(),
        }
    }
    /// Forget the text (the count stays: it numbers what was said).
    pub fn clear(&mut self) {
        self.text = None;
        self.problems = false;
    }
    pub fn as_deref(&self) -> Option<&str> {
        self.text.as_deref()
    }
    pub fn is_none(&self) -> bool {
        self.text.is_none()
    }
    pub fn is_some(&self) -> bool {
        self.text.is_some()
    }
    /// The text, owned.
    pub fn text(&self) -> Option<String> {
        self.text.clone()
    }
    /// How many errors have been said.
    pub fn seq(&self) -> u64 {
        self.seq
    }
}

impl Serialize for ErrorLine {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.text.serialize(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B167: the same error twice is two errors.
    #[test]
    fn the_same_error_twice_counts_twice() {
        let mut e = ErrorLine::default();
        e.set("refused");
        let first = e.seq();
        e.set("refused");
        assert_eq!((e.as_deref(), e.seq()), (Some("refused"), first + 1));
        e.clear();
        assert_eq!((e.is_none(), e.seq()), (true, first + 1), "clearing says nothing new");
        e.put(None);
        assert_eq!(e.seq(), first + 1);
        e.put(Some("again".into()));
        assert_eq!(e.seq(), first + 2);
        assert_eq!(serde_json::to_value(&e).unwrap(), serde_json::json!("again"));
    }
}
