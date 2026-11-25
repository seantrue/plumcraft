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
//! - `viewer.grid {popout, plane?, page?, col?, cols?}`: the plane of a 3-D or deeper array shown
//!   as a grid, its page of rows, and its window of columns ([`GRID_COLUMNS`] at a time, from a
//!   multiple of it: `col` names a column to show, `cols` steps `next`, `prev`, `first` or `last`;
//!   decision 26, N4 person gate: which columns are shown is the core's, not each shell's).
//! - `viewer.hex {popout, page?}`: the page of a value's bytes shown.
//! - `viewer.grid {…, colored?, format?}` and `viewer.table {…, colored?}` (note 26 slice 2): each
//!   cell tinted by its value from the plane's (a column's) minimum to its maximum, and an array
//!   grid's numbers in a printf format (`%.4g`, `%.3f`, `%.6f`, `%.3e`, `%d`; `%.Nf`, `%.Ne`,
//!   `%.Ng` up to 17 decimals), as the egui editor's grid and table have them.
//! - `viewer.array {popout, log?}`: an array's channel histograms on a log scale or not (B154
//!   slice 1; `popout.get`'s `stats[].histogram.heights` follow it).
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
/// Columns of an array's grid shown at once: the window `viewer.grid {col, cols}` moves.
pub const GRID_COLUMNS: u64 = 64;
/// The number formats a grid offers (any `%.Nf`, `%.Ne`, `%.Ng` up to 17, or `%d`, is taken).
pub const GRID_FORMATS: &[&str] = &["%.4g", "%.3f", "%.6f", "%.3e", "%d"];

/// A printf number format a grid takes: `%d`, or `%.N` and `f`, `e` or `g` with N at most 17.
pub fn grid_format(f: &str) -> bool {
    if f == "%d" {
        return true;
    }
    let Some(rest) = f.strip_prefix("%.") else { return false };
    let (n, kind) = rest.split_at(rest.len().saturating_sub(1));
    matches!(kind, "f" | "e" | "g") && !n.is_empty() && n.len() <= 2 && n.parse::<u8>().is_ok_and(|n| n <= 17)
}

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
    /// The first column of an array's grid shown, a multiple of [`GRID_COLUMNS`].
    pub col: u64,
    /// The page of a hex view shown.
    pub hex_page: u64,
    /// Rises with every seek or play: a shell restarts its clock from `at_ms` when it does.
    pub seq: u64,
    /// An array's channel histograms on a log scale (`viewer.array {log}`).
    pub log: bool,
    /// An array grid's cells tinted by value (`viewer.grid {colored}`).
    pub grid_colored: bool,
    /// An array grid's number format (`viewer.grid {format}`); none: the core's number text.
    pub grid_format: Option<String>,
    /// A table's numeric cells tinted by value (`viewer.table {colored}`).
    pub table_colored: bool,
    /// A table shows only its value's selected rows (`viewer.table {selectedOnly}`, note 26 slice 8).
    pub table_selected_only: bool,
    /// The part of a chart shown (`viewer.range`, note 26 slice 8).
    pub chart_range: crate::chart_view::ChartRange,
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
            col: 0,
            hex_page: 0,
            seq: 0,
            log: false,
            grid_colored: false,
            grid_format: None,
            table_colored: false,
            table_selected_only: false,
            chart_range: crate::chart_view::ChartRange::default(),
        }
    }
}

impl ViewerState {
    /// As `popout.get` reports it.
    pub fn json(&self) -> Value {
        json!({
            "table": {"sort": self.sort, "desc": self.desc, "page": self.page, "pageRows": TABLE_PAGE, "colored": self.table_colored, "selectedOnly": self.table_selected_only},
            "chart": {"range": self.chart_range},
            "tree": {"expanded": self.expanded},
            "audio": {"playing": self.playing, "at": self.at_ms as f64 / 1000.0, "loop": self.looped, "level": f64::from(self.level) / 100.0, "seq": self.seq},
            "picture": {"channel": self.channel},
            "grid": {"plane": self.plane, "page": self.page, "pageRows": TABLE_PAGE, "col": self.col, "colsShown": GRID_COLUMNS,
                     "colored": self.grid_colored, "format": self.grid_format, "formats": GRID_FORMATS},
            "hex": {"page": self.hex_page, "pageBytes": HEX_PAGE},
            "array": {"log": self.log},
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
    only(p, "viewer.table", &["sort", "desc", "page", "colored", "selectedOnly"])?;
    let colored = flag(p, "colored")?;
    let selected_only = flag(p, "selectedOnly")?;
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
    if let Some(c) = colored {
        st.table_colored = c;
    }
    if let Some(o) = selected_only {
        if o != st.table_selected_only {
            st.page = 0;
        }
        st.table_selected_only = o;
    }
    Ok(json!({"popout": id, "sort": st.sort, "desc": st.desc, "page": st.page, "colored": st.table_colored, "selectedOnly": st.table_selected_only}))
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
    only(p, "viewer.grid", &["plane", "page", "col", "cols", "colored", "format"])?;
    let colored = flag(p, "colored")?;
    let format = match p.get("format") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(f)) if grid_format(f.trim()) => Some(Some(f.trim().to_owned())),
        Some(_) => return Err(SessionError::Params("\"format\" is %d, or %.N with f, e or g (N at most 17), or null for the core's numbers".into())),
    };
    let plane = whole(p, "plane", u64::from(u32::MAX))?;
    let page = whole(p, "page", u64::from(u32::MAX))?;
    let col = whole(p, "col", u64::from(u32::MAX))?;
    let step = match p.get("cols") {
        None | Some(Value::Null) => None,
        Some(Value::String(w)) if ["next", "prev", "first", "last"].contains(&w.as_str()) => Some(w.clone()),
        Some(_) => return Err(SessionError::Params("\"cols\" is next, prev, first or last".into())),
    };
    if col.is_some() && step.is_some() {
        return Err(SessionError::Params("viewer.grid takes col or cols, not both".into()));
    }
    // the array's columns, where its shape is known before its bytes are read: the window never
    // starts past the last one (viewer.data holds it there too, for a value not yet shaped)
    let total = p.get("popout").and_then(Value::as_u64).and_then(|id| grid_columns(s, id));
    let (id, st) = state_mut(s, p)?;
    if let Some(n) = plane {
        st.plane = n;
    }
    if let Some(n) = page {
        st.page = n;
    }
    let last = total.map(last_window);
    let mut c = st.col;
    match (col, step.as_deref()) {
        (Some(n), _) => c = n - n % GRID_COLUMNS,
        (_, Some("next")) => c = c.saturating_add(GRID_COLUMNS),
        (_, Some("prev")) => c = c.saturating_sub(GRID_COLUMNS),
        (_, Some("first")) => c = 0,
        (_, Some("last")) => c = last.unwrap_or(c),
        _ => {}
    }
    st.col = last.map_or(c, |l| c.min(l));
    if let Some(c) = colored {
        st.grid_colored = c;
    }
    if let Some(f) = format {
        st.grid_format = f;
    }
    Ok(json!({"popout": id, "plane": st.plane, "page": st.page, "col": st.col, "colored": st.grid_colored, "format": st.grid_format}))
}

/// The first column of the last window of an array `cols` wide.
pub fn last_window(cols: u64) -> u64 {
    let l = cols.saturating_sub(1);
    l - l % GRID_COLUMNS
}

/// The columns of pop-out `id`'s array, from the shape its value says (`[rows, cols, …]`).
fn grid_columns(s: &Session, id: u64) -> Option<u64> {
    let (_, d) = s.run.entry(crate::run::WatchRole::Popout(id))?;
    match &d?.full.as_ref()?.meta.form {
        plumcraft_proto::data::Form::Array { shape, .. } => Some(shape.get(1).copied().unwrap_or(1)),
        _ => None,
    }
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

fn array(s: &mut Session, p: &Value) -> Result<Value> {
    only(p, "viewer.array", &["log"])?;
    let log = flag(p, "log")?;
    let (id, st) = state_mut(s, p)?;
    if let Some(b) = log {
        st.log = b;
    }
    Ok(json!({"popout": id, "log": st.log}))
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
            "{popout, sort?: column index | null, desc?: bool, page?} -> {popout, sort, desc, page}: a pop-out's table sorted by a column (null: as it came), largest first or not, and the page of 500 rows shown (a new sort shows the first page); colored: numeric cells tinted by value (viewer.data's tints); viewer.data {part: \"table\"} reads the rows",
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
            "{popout, plane?, page?, col?, cols?: \"next\" | \"prev\" | \"first\" | \"last\"} -> {popout, plane, page, col}: the plane of a 3-D or deeper array a pop-out's grid shows, its page of 500 rows, and its window of 64 columns (col: a column to show, from the multiple of 64 at or below it; cols: a step; never past the last window); colored: cells tinted by value; format: a printf number format (%.4g, %.3f, %.6f, %.3e, %d, or null for the core's); viewer.data {part: \"grid\"} reads them",
            grid,
        ),
        spec(
            "viewer.hex",
            "Bytes Page",
            "{popout, page?} -> {popout, page}: the page of 4096 bytes a pop-out's byte view shows; viewer.data {part: \"hex\"} reads them",
            hex,
        ),
        spec(
            "viewer.array",
            "Histograms on a Log Scale",
            "{popout, log?: bool} -> {popout, log}: a pop-out's array histograms drawn with counts on a log scale or not; popout.get's stats[].histogram.heights follow it",
            array,
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

    /// Decision 26 (N4 person gate): `viewer.grid {col, cols}` moves the grid's window of columns
    /// by whole windows, from multiples of 64; refuses what isn't a column or a step, and both.
    #[test]
    fn the_grids_window_of_columns_steps_and_refuses_junk() {
        let mut s = Session::new(plumcraft_proto::catalogue::SchemaReply::default());
        s.views.popouts.push(crate::view_cmds::Popout {
            id: 7,
            viewer: Some("array.grid".into()),
            tools: Default::default(),
            raised: 0,
            text: Default::default(),
            fit: None,
            state: ViewerState::default(),
            zoom_seq: 0,
            zoom_asked: None,
        });
        let mut col = |p: Value| grid(&mut s, &p).map(|r| r["col"].as_u64().unwrap_or(u64::MAX));
        assert_eq!(col(json!({"popout": 7})).unwrap(), 0);
        assert_eq!(col(json!({"popout": 7, "cols": "next"})).unwrap(), 64);
        assert_eq!(col(json!({"popout": 7, "cols": "next"})).unwrap(), 128, "no shape known yet: viewer.data holds it to the last window");
        assert_eq!(col(json!({"popout": 7, "cols": "prev"})).unwrap(), 64);
        assert_eq!(col(json!({"popout": 7, "col": 200})).unwrap(), 192, "the window that shows column 200");
        assert_eq!(col(json!({"popout": 7, "cols": "first"})).unwrap(), 0);
        assert_eq!(col(json!({"popout": 7, "cols": "prev"})).unwrap(), 0);
        for bad in [
            json!({"popout": 7, "cols": "up"}),
            json!({"popout": 7, "cols": 1}),
            json!({"popout": 7, "col": -1}),
            json!({"popout": 7, "col": 1.5}),
            json!({"popout": 7, "col": u64::MAX}),
            json!({"popout": 7, "col": 3, "cols": "next"}),
            json!({"popout": 7, "firstCol": 3}),
            json!({"popout": 8, "cols": "next"}),
        ] {
            assert!(grid(&mut s, &bad).is_err(), "{bad}");
        }
        assert_eq!(s.views.popouts[0].state.col, 0, "a refusal changes nothing");
        assert_eq!(last_window(0), 0);
        assert_eq!(last_window(64), 0);
        assert_eq!(last_window(65), 64);
        assert_eq!(last_window(150), 128);
    }

    /// B154 slice 1: `viewer.array {log}` is the histograms' log scale, refusing what isn't a
    /// flag, an unknown field, or a pop-out that isn't there; the state reports it.
    #[test]
    fn the_histograms_log_scale_is_a_command() {
        let mut s = Session::new(plumcraft_proto::catalogue::SchemaReply::default());
        s.views.popouts.push(crate::view_cmds::Popout {
            id: 3,
            viewer: Some("array.pictures".into()),
            tools: Default::default(),
            raised: 0,
            text: Default::default(),
            fit: None,
            state: ViewerState::default(),
            zoom_seq: 0,
            zoom_asked: None,
        });
        assert_eq!(s.views.popouts[0].state.json()["array"]["log"], false);
        assert_eq!(array(&mut s, &json!({"popout": 3, "log": true})).unwrap()["log"], true);
        assert_eq!(array(&mut s, &json!({"popout": 3})).unwrap()["log"], true, "nothing asked, nothing changed");
        for bad in [
            json!({"popout": 3, "log": 1}),
            json!({"popout": 3, "log": "yes"}),
            json!({"popout": 3, "scale": "log"}),
            json!({"popout": 4, "log": false}),
            json!({"log": false}),
        ] {
            assert!(array(&mut s, &bad).is_err(), "{bad}");
        }
        assert_eq!(s.views.popouts[0].state.json()["array"]["log"], true, "a refusal changes nothing");
    }

    /// Note 26 slice 2: colour by value and the number format are the core's state, refused when
    /// they aren't a flag or a printf format the grid takes.
    #[test]
    fn colour_and_format_are_commands() {
        let mut s = Session::new(plumcraft_proto::catalogue::SchemaReply::default());
        s.views.popouts.push(crate::view_cmds::Popout {
            id: 5,
            viewer: Some("array.grid".into()),
            tools: Default::default(),
            raised: 0,
            text: Default::default(),
            fit: None,
            state: ViewerState::default(),
            zoom_seq: 0,
            zoom_asked: None,
        });
        assert_eq!(grid(&mut s, &json!({"popout": 5, "colored": true, "format": "%.3f"})).unwrap()["format"], "%.3f");
        assert_eq!(table(&mut s, &json!({"popout": 5, "colored": true})).unwrap()["colored"], true);
        let st = s.views.popouts[0].state.json();
        assert_eq!((st["grid"]["colored"].as_bool(), st["grid"]["format"].as_str(), st["table"]["colored"].as_bool()), (Some(true), Some("%.3f"), Some(true)));
        assert_eq!(grid(&mut s, &json!({"popout": 5, "format": null})).unwrap()["format"], Value::Null, "null: the core's numbers");
        for bad in ["%s", "%.99f", "%.3x", "%", "%.f", "3f", "%.-1f", "%.3f;rm"] {
            assert!(grid(&mut s, &json!({"popout": 5, "format": bad})).is_err(), "{bad}");
        }
        assert!(grid(&mut s, &json!({"popout": 5, "colored": "yes"})).is_err());
        assert!(table(&mut s, &json!({"popout": 5, "colored": 1})).is_err());
        assert!(grid_format("%d") && grid_format("%.17g") && !grid_format("%.18g"));
    }

    #[test]
    fn channels_parse() {
        assert_eq!(Channel::parse("a"), Some(Channel::A));
        assert_eq!(Channel::parse("x"), None);
        assert_eq!(serde_json::to_value(Channel::Rgb).unwrap(), json!("rgb"));
    }
}
