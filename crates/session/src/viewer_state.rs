//! A pop-out viewer's state, as commands (N4 slice 5; `docs/shell-interface.md` 13.2): what the
//! person changed about how a value is shown, kept by the core so a script sets and reads it as a
//! person does, and every shell shows the same thing. None is journaled: they don't change the
//! graph.
//!
//! - `viewer.table {popout, sort?, desc?, page?}`: the column a table is sorted by (or none), the
//!   order, and the page of rows shown ([`TABLE_PAGE`] rows a page).
//! - `viewer.tree {popout, expand?, collapse?, collapseAll?}`: which members of a JSON outline are
//!   open, by JSON pointer (`""` is the root, `/a/0` member `a`'s first item).
//! - `viewer.audio {popout, play?, at?, loop?, level?}`: playing or not, the position asked for
//!   (seconds), looping, and the level (0 to 1). The clock while playing is the shell's
//!   (continuous motion, as a picture's scroll is); what is asked of it is here.
//! - `viewer.picture {popout, channel?}`: all channels, or one (`r`, `g`, `b`, `a`), or the colour
//!   without alpha (`rgb`).
//! - `viewer.grid {popout, plane?, page?}`: the plane of a 3-D or deeper array shown as a grid,
//!   and its page of rows.
//! - `viewer.hex {popout, page?}`: the page of a value's bytes shown.
//!
//! Where the line falls between a command and the shell's own motion: anything that changes what
//! is shown (a sort, a page, an outline member opened, play, a seek, a channel) is a command; the
//! scroll position, a pinch or scroll zoom of a picture inside its window, a column's width and
//! the playing clock are the shell's, as the canvas's pan and zoom are.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::{Result, Session, SessionError};

/// Rows of a table (or of an array's grid) on one page.
pub const TABLE_PAGE: u64 = 500;
/// Bytes of a hex view on one page.
pub const HEX_PAGE: u64 = 4096;
/// The most outline members kept open.
pub const MAX_EXPANDED: usize = 4096;
/// The longest JSON pointer taken.
pub const MAX_POINTER: usize = 2048;

/// A picture's channel shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Rgba,
    Rgb,
    R,
    G,
    B,
    A,
}

impl Channel {
    fn parse(s: &str) -> Option<Channel> {
        Some(match s {
            "rgba" | "all" => Channel::Rgba,
            "rgb" => Channel::Rgb,
            "r" => Channel::R,
            "g" => Channel::G,
            "b" => Channel::B,
            "a" | "alpha" => Channel::A,
            _ => return None,
        })
    }
}

/// One pop-out's viewer state (beside its text view and zoom, `view_cmds::Popout`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ViewerState {
    /// The column a table is sorted by, by index; none: as it came.
    pub sort: Option<usize>,
    /// Sorted largest first.
    pub desc: bool,
    /// The page of a table's (or grid's) rows shown, from 0.
    pub page: u64,
    /// The outline's members open, by JSON pointer.
    pub expanded: BTreeSet<String>,
    /// Playing.
    pub playing: bool,
    /// The position asked for, in milliseconds (a seek, or where play started).
    pub at_ms: u64,
    /// Play loops.
    #[serde(rename = "loop")]
    pub looped: bool,
    /// The level, in hundredths (0 to 100).
    pub level: u8,
    /// A picture's channel shown.
    pub channel: Channel,
    /// The plane of an array shown as a grid.
    pub plane: u64,
    /// The page of a hex view shown.
    pub hex_page: u64,
    /// Rises with every seek or play: a shell restarts its clock from `at_ms` when it does.
    pub seq: u64,
}

impl Default for ViewerState {
    fn default() -> Self {
        let mut expanded = BTreeSet::new();
        expanded.insert(String::new());
        ViewerState {
            sort: None,
            desc: false,
            page: 0,
            expanded,
            playing: false,
            at_ms: 0,
            looped: false,
            level: 100,
            channel: Channel::Rgba,
            plane: 0,
            hex_page: 0,
            seq: 0,
        }
    }
}

impl ViewerState {
    /// As `popout.get` reports it.
    pub fn json(&self) -> Value {
        json!({
            "table": {"sort": self.sort, "desc": self.desc, "page": self.page, "pageRows": TABLE_PAGE},
            "tree": {"expanded": self.expanded},
            "audio": {"playing": self.playing, "at": self.at_ms as f64 / 1000.0, "loop": self.looped, "level": f64::from(self.level) / 100.0, "seq": self.seq},
            "picture": {"channel": self.channel},
            "grid": {"plane": self.plane, "page": self.page, "pageRows": TABLE_PAGE},
            "hex": {"page": self.hex_page, "pageBytes": HEX_PAGE},
        })
    }
}

fn only(p: &Value, cmd: &str, allowed: &[&str]) -> Result<()> {
    if let Some(o) = p.as_object()
        && let Some(k) = o.keys().find(|k| *k != "popout" && !allowed.contains(&k.as_str()))
    {
        return Err(SessionError::Params(format!("{cmd} takes popout and {}, not {:?}", allowed.join(", "), k.chars().take(64).collect::<String>())));
    }
    Ok(())
}

fn state_mut<'a>(s: &'a mut Session, p: &Value) -> Result<(u64, &'a mut ViewerState)> {
    let id = p.get("popout").and_then(Value::as_u64).ok_or_else(|| SessionError::Params("\"popout\" (a pop-out's id) is required".into()))?;
    let o = s.views.popouts.iter_mut().find(|o| o.id == id).ok_or_else(|| SessionError::Params(format!("no pop-out {id}")))?;
    Ok((id, &mut o.state))
}

fn flag(p: &Value, k: &str) -> Result<Option<bool>> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| SessionError::Params(format!("\"{k}\" must be true or false"))),
    }
}

fn whole(p: &Value, k: &str, max: u64) -> Result<Option<u64>> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().filter(|n| *n <= max).map(Some).ok_or_else(|| SessionError::Params(format!("\"{k}\" must be a whole number from 0 to {max}"))),
    }
}

fn pointer(p: &Value, k: &str) -> Result<Option<String>> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) if t.len() <= MAX_POINTER && (t.is_empty() || t.starts_with('/')) => Ok(Some(t.clone())),
        Some(_) => Err(SessionError::Params(format!("\"{k}\" must be a JSON pointer (\"\" or \"/a/0\"), at most {MAX_POINTER} bytes"))),
    }
}

fn table(s: &mut Session, p: &Value) -> Result<Value> {
    only(p, "viewer.table", &["sort", "desc", "page"])?;
    let sort = match p.get("sort") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => Some(Some(
            v.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| *n < 1 << 16)
                .ok_or_else(|| SessionError::Params("\"sort\" is a column's index, or null".into()))?,
        )),
    };
    let desc = flag(p, "desc")?;
    let page = whole(p, "page", u64::from(u32::MAX))?;
    let (id, st) = state_mut(s, p)?;
    if let Some(c) = sort {
        if c != st.sort {
            st.page = 0;
        }
        st.sort = c;
    }
    if let Some(d) = desc {
        st.desc = d;
    }
    if let Some(n) = page {
        st.page = n;
    }
    Ok(json!({"popout": id, "sort": st.sort, "desc": st.desc, "page": st.page}))
}

fn tree(s: &mut Session, p: &Value) -> Result<Value> {
    only(p, "viewer.tree", &["expand", "collapse", "collapseAll"])?;
    let expand = pointer(p, "expand")?;
    let collapse = pointer(p, "collapse")?;
    let all = flag(p, "collapseAll")?;
    let (id, st) = state_mut(s, p)?;
    if all == Some(true) {
        st.expanded.clear();
        st.expanded.insert(String::new());
    }
    if let Some(c) = collapse {
        // closing a member closes what is open inside it
        let inside = format!("{c}/");
        st.expanded.retain(|e| *e != c && !e.starts_with(&inside));
    }
    if let Some(e) = expand {
        if st.expanded.len() >= MAX_EXPANDED && !st.expanded.contains(&e) {
            return Err(SessionError::Params(format!("at most {MAX_EXPANDED} members are open at once")));
        }
        st.expanded.insert(e);
    }
    Ok(json!({"popout": id, "expanded": st.expanded}))
}

fn audio(s: &mut Session, p: &Value) -> Result<Value> {
    only(p, "viewer.audio", &["play", "at", "loop", "level"])?;
    let play = flag(p, "play")?;
    let looped = flag(p, "loop")?;
    let at = match p.get("at") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_f64()
                .filter(|f| f.is_finite() && *f >= 0.0 && *f <= 1e7)
                .ok_or_else(|| SessionError::Params("\"at\" is a position in seconds, from 0".into()))?,
        ),
    };
    let level = match p.get("level") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_f64().filter(|f| f.is_finite()).ok_or_else(|| SessionError::Params("\"level\" is a number from 0 to 1".into()))?),
    };
    let (id, st) = state_mut(s, p)?;
    if let Some(a) = at {
        st.at_ms = (a * 1000.0).round() as u64;
        st.seq += 1;
    }
    if let Some(b) = play {
        if b && !st.playing {
            st.seq += 1;
        }
        st.playing = b;
    }
    if let Some(b) = looped {
        st.looped = b;
    }
    if let Some(l) = level {
        st.level = (l.clamp(0.0, 1.0) * 100.0).round() as u8;
    }
    Ok(json!({"popout": id, "playing": st.playing, "at": st.at_ms as f64 / 1000.0, "loop": st.looped, "level": f64::from(st.level) / 100.0, "seq": st.seq}))
}

fn picture(s: &mut Session, p: &Value) -> Result<Value> {
    only(p, "viewer.picture", &["channel"])?;
    let channel = match p.get("channel") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_str().and_then(Channel::parse).ok_or_else(|| SessionError::Params("\"channel\" is rgba, rgb, r, g, b or a".into()))?),
    };
    let (id, st) = state_mut(s, p)?;
    if let Some(c) = channel {
        st.channel = c;
    }
    Ok(json!({"popout": id, "channel": st.channel}))
}

fn grid(s: &mut Session, p: &Value) -> Result<Value> {
    only(p, "viewer.grid", &["plane", "page"])?;
    let plane = whole(p, "plane", u64::from(u32::MAX))?;
    let page = whole(p, "page", u64::from(u32::MAX))?;
    let (id, st) = state_mut(s, p)?;
    if let Some(n) = plane {
        st.plane = n;
    }
    if let Some(n) = page {
        st.page = n;
    }
    Ok(json!({"popout": id, "plane": st.plane, "page": st.page}))
}

fn hex(s: &mut Session, p: &Value) -> Result<Value> {
    only(p, "viewer.hex", &["page"])?;
    let page = whole(p, "page", u64::from(u32::MAX))?;
    let (id, st) = state_mut(s, p)?;
    if let Some(n) = page {
        st.hex_page = n;
    }
    Ok(json!({"popout": id, "page": st.hex_page}))
}

fn has_popouts(s: &Session) -> std::result::Result<(), String> {
    if s.views.popouts.is_empty() { Err("no pop-out is open".into()) } else { Ok(()) }
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    let spec = |id, label, params, run| CommandSpec { id, label, menu: &[], shortcut: None, params, enabled: has_popouts, run, journal: false };
    vec![
        spec(
            "viewer.table",
            "Sort Table",
            "{popout, sort?: column index | null, desc?: bool, page?} -> {popout, sort, desc, page}: a pop-out's table sorted by a column (null: as it came), largest first or not, and the page of 500 rows shown (a new sort shows the first page); viewer.data {part: \"table\"} reads the rows",
            table as fn(&mut Session, &Value) -> Result<Value>,
        ),
        spec(
            "viewer.tree",
            "Expand Outline",
            "{popout, expand?: JSON pointer, collapse?: JSON pointer, collapseAll?: bool} -> {popout, expanded}: open or close a member of a pop-out's JSON outline (\"\" the root, \"/a/0\" member a's first item); closing one closes what is open inside it; viewer.data {part: \"tree\"} reads the open members' children",
            tree,
        ),
        spec(
            "viewer.audio",
            "Play",
            "{popout, play?: bool, at?: seconds, loop?: bool, level?: 0..1} -> {popout, playing, at, loop, level, seq}: play or pause a pop-out's sound, seek, loop, and its level; seq rises with every seek or start, when the shell starts its clock again from at",
            audio,
        ),
        spec(
            "viewer.picture",
            "Channel",
            "{popout, channel?: \"rgba\" | \"rgb\" | \"r\" | \"g\" | \"b\" | \"a\"} -> {popout, channel}: the channel of a pop-out's picture shown (rgb: without alpha)",
            picture,
        ),
        spec(
            "viewer.grid",
            "Grid Plane",
            "{popout, plane?, page?} -> {popout, plane, page}: the plane of a 3-D or deeper array a pop-out's grid shows, and its page of 500 rows; viewer.data {part: \"grid\"} reads them",
            grid,
        ),
        spec(
            "viewer.hex",
            "Bytes Page",
            "{popout, page?} -> {popout, page}: the page of 4096 bytes a pop-out's byte view shows; viewer.data {part: \"hex\"} reads them",
            hex,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_member_closes_what_is_inside_it() {
        let mut st = ViewerState::default();
        for e in ["/a", "/a/0", "/a/0/b", "/ab"] {
            st.expanded.insert(e.into());
        }
        let inside = "/a/".to_owned();
        st.expanded.retain(|e| e != "/a" && !e.starts_with(&inside));
        assert_eq!(st.expanded.iter().cloned().collect::<Vec<_>>(), vec!["".to_owned(), "/ab".to_owned()]);
    }

    #[test]
    fn channels_parse() {
        assert_eq!(Channel::parse("a"), Some(Channel::A));
        assert_eq!(Channel::parse("x"), None);
        assert_eq!(serde_json::to_value(Channel::Rgb).unwrap(), json!("rgb"));
    }
}
