//! The Watched panel: the value a click on a node (or a wire, or an output's menu) chose, as it
//! streams in (`preview` records) and then from the run's reply ([`RunState::watched`]).
//!
//! Text shows in full when its bytes can be read, else as the engine's excerpt; a table as a grid
//! with a bar for its last numeric column; a 1-d array as a plot; a scalar large; audio as a
//! waveform, a spectrogram and a player ([`crate::audio`]). The line under
//! the summary says where the full value came from: inline, bytes from the store, or downloaded
//! by URL. A value whose bytes are gone (reaped) greys and reruns once (spec 4.6, F58).
//!
//! A list (a node run once per item of a Vary node's values) shows one item, with a stepper and
//! the list of items by label (`run.item`). A value the panel can't show (a PDF, a video, a
//! mesh: [`has_viewer`]) offers Open and Open with… (`value.open`).
//!
//! Ported from `plumcraft-view`'s Watched panel (`apps/plumcraft-view/src/main.rs`), with its
//! colours moved to theme tokens.
//!
//! [`RunState::watched`]: plumcraft_session::RunState::watched

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use plumcraft_proto::catalogue::Media;
use plumcraft_proto::data::{DataRef, Locator};
use plumcraft_proto::display::{Display, Rendition, WatchRequest};
use plumcraft_store::Blob;
use serde_json::Value;

use plumcraft_session::open_cmds::{app_name, has_viewer};

use crate::audio::AudioView;
use crate::compare_view::{CompareView, comparable, pin_bar};
use crate::media::MediaViews;
use crate::theme::Tokens;

/// The most table rows drawn.
pub const MAX_ROWS: usize = 1000;

/// Full values already in memory, by handle: the panel, its tabs, the pins and the pop-outs that
/// show one value share its bytes, which can be hundreds of megabytes, rather than each read
/// them. Held weakly: a value goes when the last viewer lets it go.
#[derive(Clone, Default)]
pub struct Fulls(Arc<Mutex<HashMap<String, Weak<Blob>>>>);

impl Fulls {
    /// `r`'s bytes: those already here, else read with `resolve`.
    pub fn read(&self, r: &DataRef, resolve: &dyn Fn(&DataRef) -> Option<Blob>) -> Option<Arc<Blob>> {
        let Some(key) = r.handle.as_ref().map(ToString::to_string) else { return resolve(r).map(Arc::new) };
        let Ok(mut held) = self.0.lock() else { return resolve(r).map(Arc::new) };
        if let Some(b) = held.get(&key).and_then(Weak::upgrade) {
            return Some(b);
        }
        let b = Arc::new(resolve(r)?);
        held.retain(|_, w| w.strong_count() > 0);
        held.insert(key, Arc::downgrade(&b));
        Some(b)
    }
}

/// What the panel has read of the watched value.
#[derive(Debug, Clone, Default)]
pub struct Watched {
    /// The display last taken (to see when a new one arrives).
    seen: Option<Display>,
    /// The display's `full` value, read once when the display arrives.
    pub full: Option<Arc<Blob>>,
    /// The watched value's blob was gone.
    pub missing: bool,
    /// A rerun was asked for because of it (once per value, F58).
    reran_for_missing: bool,
}

impl Watched {
    /// Forget everything (a new graph).
    pub fn clear(&mut self) {
        *self = Watched::default();
    }

    /// Take the session's watched display, reading its full value with `resolve` when it is
    /// new. True when the value's bytes are gone and a rerun should publish them again (once).
    pub fn update(&mut self, d: Option<&Display>, resolve: &dyn Fn(&DataRef) -> Option<Arc<Blob>>) -> bool {
        if self.seen.as_ref() == d {
            return false;
        }
        let Some(d) = d else {
            self.seen = None;
            self.full = None;
            self.missing = false;
            return false;
        };
        // A document is never read: the UI doesn't decode it (spec 10.1), and it may be huge.
        let readable = d.media != Media::Doc;
        // no handle (an array published by path alone): its locator says which value it is
        let id = |f: &DataRef| f.handle.as_ref().map_or_else(|| format!("{:?}", f.at), ToString::to_string);
        let same = self.seen.as_ref().and_then(|w| w.full.as_ref()).map(id) == d.full.as_ref().map(id);
        if !same || self.full.is_none() {
            self.full = d.full.as_ref().filter(|_| readable).and_then(resolve);
        }
        self.missing = readable && d.full.is_some() && self.full.is_none();
        self.seen = Some(d.clone());
        if self.missing && !self.reran_for_missing {
            self.reran_for_missing = true;
            return true;
        }
        if !self.missing {
            self.reran_for_missing = false;
        }
        false
    }

    /// The panel. `watch` is what is watched, `display` its latest display, `running` whether
    /// a run is in flight.
    /// `audio` is the audio view's state (an audio value's waveform, spectrogram and player);
    /// `media` the picture viewers (images, SVG, documents: [`crate::media`]); `opens` the
    /// apps chosen before with Open with… and the latest open's outcome. Returns the commands
    /// its buttons ask for (`run.item`, `value.open`; `value.open` with `choose` asks the host
    /// for an app first).
    /// `pane` draws the viewers the panel doesn't have its own of (array values, JSON tree, PDF
    /// pages, bytes: [`crate::viewers`]), shown as tabs beside its own; `chosen` is the tab
    /// picked (`view.viewer`). `save` is present when the engine can write this value: `Ok` to
    /// offer it, `Err` for why not now (`run.save`, which asks where first). `compare` is the
    /// compare view, which takes the panel's place while it shows; `pinned`, whether anything is.
    #[allow(clippy::too_many_arguments)]
    pub fn ui(
        &self,
        ui: &mut egui::Ui,
        watch: Option<&WatchRequest>,
        display: Option<&Display>,
        running: bool,
        tokens: &Tokens,
        audio: &mut AudioView,
        media: &mut MediaViews,
        opens: &Opens<'_>,
        pane: &mut crate::viewers::Pane,
        chosen: Option<&str>,
        save: Option<Result<(), String>>,
        compare: &mut CompareView,
        pinned: bool,
    ) -> Vec<(&'static str, Value)> {
        let mut asks = Vec::new();
        ui.label(egui::RichText::new("Watched").heading().color(tokens.text));
        let Some(w) = watch else {
            ui.label(egui::RichText::new("Click a node's title to watch its output.").color(tokens.text_dim));
            return asks;
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{}.{}", w.node, w.port)).monospace());
            if ui.small_button("Pop out").on_hover_text("Open this value in a window of its own, updated by every run (view.popout)").clicked() {
                asks.push(("view.popout", serde_json::json!({"node": w.node, "port": w.port, "index": w.index, "viewer": chosen})));
            }
        });
        let Some(d) = display else {
            if compare.is_on() {
                asks.extend(compare.ui(ui, media.array.log, tokens));
                return asks;
            }
            let text = if running { "waiting for the value…" } else { "not run yet: run to see it" };
            ui.label(egui::RichText::new(text).color(tokens.text_dim));
            return asks;
        };
        if let Some(e) = &d.error {
            ui.colored_label(tokens.error, &e.message);
            if compare.is_on() {
                asks.extend(compare.ui(ui, media.array.log, tokens));
            }
            return asks;
        }
        if d.items > 1 {
            list_bar(ui, w, d, running, tokens, &mut asks);
        }
        ui.label(egui::RichText::new(&d.summary).color(tokens.text_dim));
        if let Some(allowed) = save {
            save_bar(ui, w, d, &allowed, &mut asks);
        }
        if comparable(d) || pinned {
            pin_bar(ui, pinned, compare.is_on(), &mut asks);
        }
        if compare.is_on() {
            ui.separator();
            asks.extend(compare.ui(ui, media.array.log, tokens));
            return asks;
        }
        if let Some(recipe) = d.recipe.as_ref().filter(|r| r.is_object()) {
            asks.extend(crate::recipe_view::ui(ui, recipe, tokens));
        }
        // The viewers this value offers, as tabs; the panel draws its own kinds itself.
        let kinds = crate::viewers::kinds_for(d);
        let current = chosen.and_then(crate::viewers::kind).filter(|k| kinds.iter().any(|x| x.id == k.id)).or_else(|| kinds.first().copied());
        if kinds.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                for k in &kinds {
                    let on = current.is_some_and(|c| c.id == k.id);
                    if ui.selectable_label(on, k.label).on_hover_text(format!("view.viewer {{viewer: {:?}}}", k.id)).clicked() && !on {
                        asks.push(("view.viewer", serde_json::json!({"viewer": k.id})));
                    }
                }
            });
        }
        let own = current.is_none_or(|k| k.panel_builtin);
        if !has_viewer(d.media) && d.full.is_some() && own {
            open_bar(ui, w, opens, &mut asks);
        }
        if let Some(f) = &d.full {
            let how = match (&self.full, f.at.first()) {
                (Some(_), Some(Locator::Inline { .. })) => "inline".to_owned(),
                (Some(_), Some(Locator::Url { .. })) => format!("{} bytes downloaded by URL, verified, kept in the UI's CAS", f.meta.size),
                (Some(_), _) => format!("{} bytes from the store", f.meta.size),
                (None, _) if d.media == Media::Doc && f.at.is_empty() => format!("{} bytes, on the engine's machine", f.meta.size),
                (None, _) if d.media == Media::Doc => format!("{} bytes, in the engine's CAS", f.meta.size),
                (None, _) => "gone (reaped); rerun to publish it again".to_owned(),
            };
            let handle = f.handle.as_ref().map(|h| h.to_string()).unwrap_or_default();
            ui.label(egui::RichText::new(format!("{how} · {}", handle.get(..20).unwrap_or(&handle))).small().color(tokens.text_faint)).on_hover_text(handle);
        }
        ui.separator();
        if !own {
            asks.extend(pane.content_ui(ui, tokens));
            return asks;
        }
        egui::ScrollArea::vertical().id_salt("plumcraft-watched-scroll").auto_shrink([false, false]).show(ui, |ui| match d.media {
            Media::Audio => audio.ui(ui, d, self.missing, tokens),
            Media::Image | Media::Vector | Media::Doc if let Some(v) = media.viewer(d.media) => {
                v.ui(ui, tokens);
                asks.extend(v.asks());
            }
            Media::Table => {
                let table = d.value.clone().or_else(|| self.full.as_ref().and_then(|b| serde_json::from_slice(&b.bytes).ok()));
                match table {
                    Some(t) => table_view(ui, &t, tokens),
                    None => {
                        ui.label(egui::RichText::new("no table").color(tokens.text_dim));
                    }
                }
            }
            Media::Ndarray if media.array.has_pictures() => {
                asks.extend(media.array.view_bar(ui, d, tokens));
                asks.extend(media.array.pictures_ui(ui, tokens));
                media.array.stats_ui(ui, d, tokens);
            }
            Media::Ndarray => {
                match self.full.as_deref().and_then(numbers) {
                    Some(v) => plot(ui, &v, tokens),
                    None => {
                        ui.label(egui::RichText::new("no values").color(tokens.text_dim));
                    }
                }
                media.array.stats_ui(ui, d, tokens);
            }
            _ => {
                let full_text = self.full.as_ref().filter(|_| d.media == Media::Text).map(|b| String::from_utf8_lossy(&b.bytes).into_owned());
                match full_text {
                    Some(t) => {
                        ui.label(egui::RichText::new(t).monospace());
                    }
                    None => {
                        for r in &d.renditions {
                            if let Rendition::Excerpt { text, .. } = r {
                                ui.label(egui::RichText::new(text).monospace().color(if self.missing { tokens.text_faint } else { tokens.text }));
                            }
                        }
                    }
                }
                if let Some(v) = &d.value {
                    let shown = v.as_str().map_or_else(|| v.to_string(), str::to_owned);
                    ui.label(egui::RichText::new(shown).monospace().size(28.0));
                }
            }
        });
        asks
    }
}

/// What the Open buttons need: the apps chosen before (most recent first), and the latest
/// open's outcome to show under them.
#[derive(Debug, Default)]
pub struct Opens<'a> {
    pub recent_apps: &'a [std::path::PathBuf],
    pub note: Option<String>,
}

/// A list's stepper (previous, "item i of n", next) and its items by label.
fn list_bar(ui: &mut egui::Ui, w: &WatchRequest, d: &Display, running: bool, tokens: &Tokens, asks: &mut Vec<(&'static str, Value)>) {
    let n = d.items as i64;
    let at = d.index.clamp(0, n - 1);
    ui.horizontal(|ui| {
        if ui.add_enabled(!running && at > 0, egui::Button::new("◀")).on_hover_text("The previous item").clicked() {
            asks.push(("run.item", serde_json::json!({"index": at - 1})));
        }
        ui.label(egui::RichText::new(format!("item {} of {n}", at + 1)).color(tokens.text));
        if ui.add_enabled(!running && at + 1 < n, egui::Button::new("▶")).on_hover_text("The next item").clicked() {
            asks.push(("run.item", serde_json::json!({"index": at + 1})));
        }
        ui.separator();
        let hover = "Every item side by side, each labelled with what made it; click one to show it (viewer.sheet)";
        if ui.add_enabled(!running, egui::Button::selectable(w.sheet, "Sheet")).on_hover_text(hover).clicked() {
            asks.push(("viewer.sheet", serde_json::json!({"on": !w.sheet})));
        }
    });
    egui::CollapsingHeader::new(format!("All {n} items")).id_salt("plumcraft-watched-items").default_open(n <= 12).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("plumcraft-watched-items-scroll").max_height(160.0).show(ui, |ui| {
            for (j, cell) in d.sheet.iter().enumerate() {
                let text = match (cell.label.is_empty(), cell.summary.is_empty()) {
                    (false, false) => format!("{}  —  {}", cell.label, cell.summary),
                    (false, true) => cell.label.clone(),
                    _ => cell.summary.clone(),
                };
                let r = ui
                    .add_enabled(!running, egui::Button::selectable(j as i64 == at, egui::RichText::new(format!("{:>3}  {text}", j + 1)).monospace().small()));
                if r.clicked() && j as i64 != at {
                    asks.push(("run.item", serde_json::json!({"index": j})));
                }
            }
            if (d.sheet.len() as u64) < d.items {
                ui.label(egui::RichText::new(format!("… and {} more", d.items - d.sheet.len() as u64)).small().color(tokens.text_faint));
            }
        });
    });
}

/// Save this value (the item shown, when it is a list), and every item on one contact sheet.
fn save_bar(ui: &mut egui::Ui, w: &WatchRequest, d: &Display, allowed: &Result<(), String>, asks: &mut Vec<(&'static str, Value)>) {
    use crate::canvas::{CONTACT_SHEET, SAVE_HELP, SAVE_RESULT, SHEET_HELP};
    let button = |ui: &mut egui::Ui, label: &str, help: &str| {
        let r = ui.add_enabled(allowed.is_ok(), egui::Button::new(label));
        match allowed {
            Ok(()) => r.on_hover_text(help),
            Err(why) => r.on_disabled_hover_text(why),
        }
        .clicked()
    };
    ui.horizontal(|ui| {
        if button(ui, SAVE_RESULT, SAVE_HELP) {
            let index = (d.items > 1).then(|| d.index.clamp(0, d.items as i64 - 1));
            asks.push(("run.save", serde_json::json!({"node": w.node, "port": w.port, "index": index, "ask": true})));
        }
        if d.items > 1 && button(ui, CONTACT_SHEET, SHEET_HELP) {
            asks.push(("run.save", serde_json::json!({"node": w.node, "port": w.port, "sheet": true, "ask": true})));
        }
    });
}

/// Open (the system's default app) and Open with… (an app chosen before, or another).
fn open_bar(ui: &mut egui::Ui, w: &WatchRequest, opens: &Opens<'_>, asks: &mut Vec<(&'static str, Value)>) {
    let target = serde_json::json!({"node": w.node, "port": w.port});
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("No viewer here.").small());
        if ui.button("Open").on_hover_text("Open a copy in the default app for its type").clicked() {
            asks.push(("value.open", target.clone()));
        }
        ui.menu_button("Open with…", |ui| {
            for app in opens.recent_apps {
                if ui.button(app_name(app)).on_hover_text(app.display().to_string()).clicked() {
                    let mut p = target.clone();
                    p["app"] = Value::from(app.display().to_string());
                    asks.push(("value.open", p));
                    ui.close();
                }
            }
            if !opens.recent_apps.is_empty() {
                ui.separator();
            }
            if ui.button("Choose an App…").clicked() {
                let mut p = target.clone();
                p["choose"] = Value::Bool(true);
                asks.push(("value.open", p));
                ui.close();
            }
        });
    });
    if let Some(note) = &opens.note {
        ui.label(egui::RichText::new(note).small()).on_hover_text(note);
    }
}

/// A table (`{"columns": [{name, type, values}]}`) as a grid, with a bar for the last numeric
/// column.
pub fn table_view(ui: &mut egui::Ui, t: &Value, tokens: &Tokens) {
    let cols = t.get("columns").and_then(Value::as_array).cloned().unwrap_or_default();
    let values = |c: &Value| c.get("values").and_then(Value::as_array).cloned().unwrap_or_default();
    let rows = cols.first().map_or(0, |c| values(c).len());
    let bars = cols.iter().rev().find(|c| c.get("values").and_then(Value::as_array).is_some_and(|v| v.iter().all(Value::is_number)));
    let bar_values = bars.map(values).unwrap_or_default();
    let max = bar_values.iter().filter_map(Value::as_f64).filter(|f| f.is_finite()).fold(0.0, f64::max);
    egui::Grid::new("plumcraft-watched-table").striped(true).show(ui, |ui| {
        for c in &cols {
            ui.label(egui::RichText::new(c.get("name").and_then(Value::as_str).unwrap_or("?")).strong());
        }
        ui.end_row();
        for r in 0..rows.min(MAX_ROWS) {
            for c in &cols {
                let v = c.get("values").and_then(|v| v.get(r)).cloned().unwrap_or(Value::Null);
                ui.label(v.as_str().map_or_else(|| v.to_string(), str::to_owned));
            }
            if let Some(b) = bar_values.get(r).and_then(Value::as_f64).filter(|b| max > 0.0 && b.is_finite()) {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(120.0, 12.0), egui::Sense::hover());
                let w = rect.width() * (b / max).clamp(0.0, 1.0) as f32;
                ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(w, rect.height())), 2.0, tokens.chart);
            }
            ui.end_row();
        }
    });
    if rows > MAX_ROWS {
        ui.label(egui::RichText::new(format!("… {} more rows", rows - MAX_ROWS)).color(tokens.text_dim));
    }
}

/// A 1-d array's values as numbers, for the dtypes the text engine makes and a few more.
pub fn numbers(b: &Blob) -> Option<Vec<f64>> {
    if b.shape.len() != 1 {
        return None;
    }
    let v: Vec<f64> = match b.dtype.as_str() {
        "|u1" => b.bytes.iter().map(|x| f64::from(*x)).collect(),
        "<u4" => b.bytes.as_chunks::<4>().0.iter().map(|c| f64::from(u32::from_le_bytes(*c))).collect(),
        "<i4" => b.bytes.as_chunks::<4>().0.iter().map(|c| f64::from(i32::from_le_bytes(*c))).collect(),
        "<f4" => b.bytes.as_chunks::<4>().0.iter().map(|c| f64::from(f32::from_le_bytes(*c))).collect(),
        "<f8" => b.bytes.as_chunks::<8>().0.iter().map(|c| f64::from_le_bytes(*c)).collect(),
        _ => return None,
    };
    Some(v)
}

/// A 1-d plot: a bar per value when they fit, otherwise each pixel column spans the min and max
/// of its values; the range is labelled.
pub fn plot(ui: &mut egui::Ui, v: &[f64], tokens: &Tokens) {
    let (lo, hi) = v.iter().filter(|x| x.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(*x), b.max(*x)));
    ui.label(egui::RichText::new(format!("{} values, {lo} … {hi}", v.len())).small().color(tokens.text_dim));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(100.0), 160.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_stroke(rect, 2.0, egui::Stroke::new(1.0, tokens.separator), egui::StrokeKind::Inside);
    if v.is_empty() || !lo.is_finite() || !hi.is_finite() {
        return;
    }
    // Bars rise from zero when the range includes it, else from the minimum.
    let base = if lo <= 0.0 && hi >= 0.0 { 0.0 } else { lo };
    let (top, bottom) = (hi.max(base), lo.min(base));
    let span = if top > bottom { top - bottom } else { 1.0 };
    let y = |x: f64| rect.bottom() - 4.0 - ((x.clamp(bottom, top) - bottom) / span) as f32 * (rect.height() - 8.0);
    let inner = rect.shrink2(egui::vec2(4.0, 0.0));
    let px = inner.width().max(1.0) as usize;
    if v.len() <= px {
        let w = inner.width() / v.len() as f32;
        for (i, x) in v.iter().enumerate().filter(|(_, x)| x.is_finite()) {
            let left = inner.left() + i as f32 * w;
            let (a, b) = (y(*x).min(y(base)), y(*x).max(y(base)));
            p.rect_filled(egui::Rect::from_min_max(egui::pos2(left + w * 0.1, a), egui::pos2(left + w * 0.9, b.max(a + 1.0))), 1.0, tokens.chart);
        }
        return;
    }
    for col in 0..px {
        let (a, b) = (col * v.len() / px, ((col + 1) * v.len() / px).max(col * v.len() / px + 1));
        let slice = v.get(a..b.min(v.len())).unwrap_or_default();
        let (mn, mx) = slice.iter().filter(|x| x.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(m, n), x| (m.min(*x), n.max(*x)));
        if !mn.is_finite() {
            continue;
        }
        let x = inner.left() + col as f32 + 0.5;
        p.line_segment([egui::pos2(x, y(mn) + 0.5), egui::pos2(x, y(mx) - 0.5)], egui::Stroke::new(1.0, tokens.chart));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_read_the_engines_dtypes() {
        let b = Blob { dtype: "<u4".into(), shape: vec![2], bytes: [65u32, 66].iter().flat_map(|c| c.to_le_bytes()).collect::<Vec<u8>>().into() };
        assert_eq!(numbers(&b), Some(vec![65.0, 66.0]));
        assert_eq!(numbers(&Blob { dtype: "<c8".into(), shape: vec![1], bytes: (vec![0; 8]).into() }), None);
        assert_eq!(numbers(&Blob { dtype: "|u1".into(), shape: vec![1, 1], bytes: (vec![0]).into() }), None, "1-d only");
    }

    /// An array published by path has no handle: two of them are two values, not one.
    #[test]
    fn values_without_handles_are_told_apart_by_where_they_are() {
        let at = |path: &str| Display { full: Some(serde_json::from_value(json!({"at": [{"kind": "path", "path": path}]})).unwrap()), ..Default::default() };
        let read = |n: u8| Arc::new(Blob { dtype: "|u1".into(), shape: vec![1], bytes: vec![n].into() });
        let mut w = Watched::default();
        w.update(Some(&at("/a.npy")), &|_| Some(read(1)));
        w.update(Some(&at("/b.npy")), &|_| Some(read(2)));
        assert_eq!(&*w.full.unwrap().bytes, &[2], "the second value's bytes");
    }

    #[test]
    fn a_gone_value_reruns_once() {
        let d = Display {
            full: Some(serde_json::from_value(json!({"handle": "md5:raw:9e107d9d372bb6826bd81d3542a419d6", "at": []})).unwrap()),
            ..Default::default()
        };
        let mut w = Watched::default();
        assert!(w.update(Some(&d), &|_| None), "gone: rerun");
        assert!(w.missing);
        assert!(!w.update(Some(&d), &|_| None), "the same display: nothing more");
        let mut d2 = d.clone();
        d2.summary = "again".into();
        assert!(!w.update(Some(&d2), &|_| None), "still gone after the rerun: say so, don't loop");
        let bytes = Arc::new(Blob { dtype: "|u1".into(), shape: vec![2], bytes: (b"hi".to_vec()).into() });
        let mut d3 = d.clone();
        d3.summary = "back".into();
        assert!(!w.update(Some(&d3), &|_| Some(bytes.clone())));
        assert!(!w.missing && w.full.is_some());
        assert!(!w.update(None, &|_| None));
        assert!(w.full.is_none());
    }

    #[test]
    fn hostile_tables_and_plots_draw() {
        let ctx = egui::Context::default();
        let tokens = crate::theme::DEFAULT;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            for t in [
                json!({}),
                json!({"columns": 3}),
                json!({"columns": [{"values": [1, "x", null]}, {"name": 7}]}),
                json!({"columns": [{"values": [1e308, -1e308]}]}),
            ] {
                table_view(ui, &t, &tokens);
            }
            for v in [vec![], vec![f64::NAN], vec![f64::INFINITY, 1.0], (0..5000).map(f64::from).collect()] {
                plot(ui, &v, &tokens);
            }
        });
        out.textures_delta.clear();
    }
}
