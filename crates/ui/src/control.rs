//! The egui window's side of the control channel (`docs/control-protocol.md`).
//!
//! The dispatcher is the editor core's ([`plumcraft_editor_core::control`], N1 slice 5): every
//! method, the parked replies and engine switching are answered there, the same for every shell.
//! What stays here is what only this window can do, as its [`core::Shell`]: its part of
//! `ui.inspect` (the canvas, selection, windows, popups, the Watched panel's views), `ui.set`
//! (the canvas's transform, the palette, group focus), `ui.audio` (the audio device) and
//! `ui.screenshot` (pixels).
//!
//! The transport (`plumcraft-automation`) queues requests; the app drains them at the start of
//! every frame ([`EditorApp::drain_control`]) and hands each to [`EditorApp::handle_control`]
//! with a reply callback. Most reply at once; a screenshot replies a few frames later, when the
//! window delivers the image, and a run wait when the run finishes. Every request gets exactly
//! one reply: a window that never delivers its screenshot (a locked screen) is drawn offscreen
//! instead ([`crate::offscreen`]) after [`WINDOW_SHOT_WAIT`].
//!
//! Lifted from the research software's `crates/ui-egui/src/control.rs` (2026-10-27, the research software `8d96021`; plan
//! 10.3, E1.4) without the viewer or the parameters panel; E2 added the file and undo state to
//! `ui.inspect`, and `ui.invoke` and `ui.prompt`; E3 added `groups` to `ui.inspect` and `focus`
//! to `ui.set`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::emath::TSTransform;
use plumcraft_editor_core::control::{self as core, Parked, ShotRequest, View, check_fields};
use serde_json::{Map, Value, json};

use crate::EditorApp;

pub use plumcraft_editor_core::control::{Reply, WAIT_RUN_MAX_S, default_screenshot_path, err, ok};

/// Frames to wait before capturing, so layout and fades settle.
const SETTLE_FRAMES: u32 = 3;

/// How long the window has to deliver a screenshot before the app draws one offscreen.
pub const WINDOW_SHOT_WAIT: Duration = Duration::from_secs(2);

struct Shot {
    token: u64,
    path: PathBuf,
    frames: u32,
    /// When the window was asked (None: not yet).
    requested_at: Option<Instant>,
    /// Skip the window and draw offscreen.
    offscreen: bool,
    reply: Reply,
}

/// Screenshots in flight, and the replies the core parked (engine switches, run waits).
pub struct ControlState {
    shots: Vec<Shot>,
    /// What the core's dispatcher holds for later ([`plumcraft_editor_core::control::Parked`]).
    pub parked: Parked,
    /// How long a window screenshot may take before the offscreen fallback ([`WINDOW_SHOT_WAIT`]).
    pub window_shot_wait: Duration,
}

impl Default for ControlState {
    fn default() -> Self {
        ControlState { shots: Vec::new(), parked: Parked::default(), window_shot_wait: WINDOW_SHOT_WAIT }
    }
}

impl ControlState {
    /// Requests still waiting for their reply.
    pub fn pending(&self) -> usize {
        self.shots.len() + self.parked.pending()
    }
}

/// What only this window can do for the control channel: its canvas, windows and pixels.
impl core::Shell for EditorApp {
    fn view(&self) -> View<'_> {
        View {
            link: self.doc.link.as_ref(),
            state: &self.doc.engine,
            coordinator: self.doc.coordinator.as_ref(),
            switching: &self.doc.switching,
            last_error: self.doc.last_error.as_deref(),
            asks: &self.doc.asks,
            document: &self.doc.document,
            app_title: self.app_title,
            run_anim: Some(&self.run_anim),
            prefs: Some(&self.prefs),
            theme: Some(self.tokens.name),
        }
    }
    fn parked(&mut self) -> &mut Parked {
        &mut self.control.parked
    }
    fn clear_last_error(&mut self) {
        self.doc.last_error = None;
    }
    /// What the command starts on an engine goes to the worker meanwhile, as in a frame.
    fn request<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let outside = !std::mem::replace(&mut self.in_frame, true);
        self.engine_asked = false;
        let r = f(self);
        if outside {
            self.in_frame = false;
        }
        r
    }
    fn engine_asked(&self) -> bool {
        self.engine_asked
    }
    fn presented(&mut self) {
        self.present_asks();
    }
    fn redraw(&self) {
        if let Some(ctx) = self.control_ctx() {
            ctx.request_repaint();
        }
    }
    fn inspect_shell(&self) -> Map<String, Value> {
        self.shell_inspect(self.control_ctx())
    }
    fn screenshot(&mut self, shot: ShotRequest, reply: Reply) {
        let token = self.control.parked.next_token();
        let path = shot.path.unwrap_or_else(|| default_screenshot_path(token));
        self.control.shots.push(Shot { token, path, frames: 0, requested_at: None, offscreen: shot.offscreen, reply });
        core::Shell::redraw(self);
    }
    fn ui_set(&mut self, params: &Value) -> Result<Value, String> {
        self.apply_ui_set(params)?;
        Ok(self.shell_inspect(self.control_ctx()).remove("canvas").unwrap_or(Value::Null))
    }
    fn ui_audio(&mut self, params: &Value) -> Result<Value, String> {
        self.apply_ui_audio(params)?;
        Ok(self.audio.inspect())
    }
}

impl EditorApp {
    /// Hand every queued control request to [`EditorApp::handle_control`] (start of a frame).
    /// A request the transport stopped waiting for is answered with a timeout, not run.
    pub fn drain_control(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.control_rx else { return };
        let requests: Vec<plumcraft_automation::server::ControlRequest> = rx.try_iter().collect();
        for req in requests {
            if req.expired() {
                let _ = req.reply.send(err("timeout: the request expired before it ran"));
                continue;
            }
            let reply = req.reply.clone();
            self.handle_control(
                ctx,
                &req.method,
                &req.params,
                Box::new(move |v: Value| {
                    let _ = reply.send(v);
                }),
            );
        }
    }

    /// Handle one control request ([`plumcraft_editor_core::control::handle`]). `reply` is
    /// called exactly once, now or a few frames later.
    pub fn handle_control(&mut self, ctx: &egui::Context, method: &str, params: &Value, reply: Reply) {
        self.control_ctx = Some(ctx.clone());
        core::handle(self, method, params, reply);
    }

    /// Handle a request that replies at once ([`plumcraft_editor_core::control::now`]).
    pub fn control_now(&mut self, ctx: &egui::Context, method: &str, params: &Value) -> Value {
        self.control_ctx = Some(ctx.clone());
        core::now(self, method, params)
    }

    /// The context control replies read (what popups are open) and repaint.
    fn control_ctx(&self) -> Option<&egui::Context> {
        self.control_ctx.as_ref().or(self.repaint.as_ref())
    }

    /// The UI state as JSON: the core's parts ([`plumcraft_editor_core::control::inspect_core`])
    /// and this window's.
    pub fn inspect(&self, ctx: &egui::Context) -> Value {
        let mut m = core::inspect_core(self);
        m.extend(self.shell_inspect(Some(ctx)));
        Value::Object(m)
    }

    /// `ui.inspect`'s parts only this window knows: the canvas, the selection, its windows and
    /// popups, the Watched panel's views, pop-outs and wire previews, what was copied.
    fn shell_inspect(&self, ctx: Option<&egui::Context>) -> Map<String, Value> {
        let g = self.doc.session.graph();
        let t = self.canvas.to_global;
        let r = self.canvas.rect;
        let popups = json!({
            "palette": self.canvas.palette.as_ref().map(|p| json!({"query": p.query, "selected": p.selected, "at": [p.screen_pos.x, p.screen_pos.y]})),
            "anyPopup": ctx.is_some_and(egui::Popup::is_any_open),
        });
        let run = &self.doc.session.run;
        let watched = run.panel().map(|(w, d)| {
            // the application's value for the node stands in for the run's (crate::app_hooks)
            let d = self.app_display.as_ref().or(d);
            json!({"node": w.node, "port": w.port, "summary": d.map(|d| d.summary.clone()), "full": self.watched.full.is_some(), "missing": self.watched.missing,
                   "audio": d.filter(|d| d.media == plumcraft_proto::catalogue::Media::Audio).map(|_| self.audio.inspect()),
                   "media": d.map(|d| d.media),
                   "view": d.and_then(|d| self.media.inspect(d.media)),
                   "viewer": self.panel_pane.current(),
                   "panelViewer": self.panel_pane.inspect(),
                   "sheet": w.sheet,
                   "px": w.px,
                   "compare": self.compare.inspect()})
        });
        let v = json!({
            "popouts": self.popouts_inspect(),
            "parameters": self.doc.session.views.parameters.as_ref().map(|n| json!({"node": n, "fields": self.params.rects.iter().map(|(k, _)| k).collect::<Vec<_>>()})),
            "editors": self.value_editors.inspect(),
            "wirePreview": self.wires.inspect(),
            "copied": self.copied.is_some(),
            "selection": self.canvas.selection(),
            "canvas": {"zoom": t.scaling, "pan": [t.translation.x, t.translation.y],
                       "rect": if r.is_finite() { json!([r.min.x, r.min.y, r.max.x, r.max.y]) } else { Value::Null },
                       "editing": self.canvas.edits.is_editing()},
            "popups": popups,
            "watched": watched,
            "groups": crate::groups_view::inspect(&self.canvas, g),
            "rename": self.rename.as_ref().map(rename_inspect),
        });
        match v {
            Value::Object(m) => m,
            _ => Map::new(),
        }
    }

    /// `ui.audio {play?, loop?}`: play or stop the watched audio, loop it or not. The reply is
    /// the audio view's state (as `ui.inspect`'s `watched.audio`).
    fn apply_ui_audio(&mut self, params: &Value) -> Result<(), String> {
        check_fields(params, &["play", "loop"])?;
        if let Some(l) = params.get("loop") {
            self.audio.player.set_looping(l.as_bool().ok_or("loop must be a boolean")?);
        }
        if let Some(p) = params.get("play") {
            let on = p.as_bool().ok_or("play must be a boolean")?;
            self.audio.set_playing(on)?;
        }
        Ok(())
    }

    /// `ui.set {zoom?, pan?, fit?, palette?, focus?}` (`focus`: a group id to enter, or null to
    /// leave it).
    fn apply_ui_set(&mut self, params: &Value) -> Result<(), String> {
        check_fields(params, &["zoom", "pan", "fit", "palette", "focus"])?;
        if let Some(f) = params.get("focus") {
            crate::groups_view::set_focus(&mut self.canvas, self.doc.session.graph(), f)?;
        }
        let mut t = self.canvas.to_global;
        let zoom = params.get("zoom").map(|z| z.as_f64().filter(|z| z.is_finite() && (0.05..=8.0).contains(z)).ok_or("zoom must be a number in 0.05..8"));
        let pan = params.get("pan").map(|p| {
            let a = p.as_array().filter(|a| a.len() == 2).ok_or("pan must be [x, y]")?;
            let c = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|f| f.is_finite()).map(|f| f as f32).ok_or("pan must be [x, y] numbers");
            Ok::<_, &str>(egui::vec2(c(a.first())?, c(a.get(1))?))
        });
        if let Some(z) = zoom {
            let z = z? as f32;
            // Zoom about the canvas centre.
            let center = if self.canvas.rect.is_finite() { self.canvas.rect.center() } else { egui::Pos2::ZERO };
            let graph_center = t.inverse() * center;
            t = TSTransform { scaling: z, translation: center.to_vec2() - graph_center.to_vec2() * z };
        }
        if let Some(p) = pan {
            t.translation = p?;
        }
        if zoom.is_some() || pan.is_some() {
            self.canvas.set_transform = Some(t);
        }
        if let Some(fit) = params.get("fit")
            && fit.as_bool().ok_or("fit must be a boolean")?
        {
            self.canvas.request_fit();
        }
        if let Some(p) = params.get("palette") {
            let center = if self.canvas.rect.is_finite() { self.canvas.rect.center() } else { egui::Pos2::ZERO };
            match p {
                Value::Bool(false) | Value::Null => self.canvas.palette = None,
                Value::Bool(true) => self.canvas.open_palette(center),
                Value::String(q) => {
                    if self.canvas.palette.is_none() {
                        self.canvas.open_palette(center);
                    }
                    if let Some(state) = &mut self.canvas.palette {
                        state.query = q.chars().take(256).collect();
                        state.selected = 0;
                    }
                }
                _ => return Err("palette must be a boolean or a search string".into()),
            }
        }
        Ok(())
    }
}

/// `ui.inspect`'s `rename`: the rename window, while it is open.
fn rename_inspect(d: &crate::rename_view::RenameDialog) -> Value {
    let (kind, id) = match &d.target {
        crate::rename_view::RenameTarget::Node(n) => ("node", n.0.as_str()),
        crate::rename_view::RenameTarget::Frame(f) => ("frame", f.0.as_str()),
    };
    json!({"kind": kind, "id": id, "text": d.text})
}

/// End of frame: answer finished run waits, ask the window for settled screenshots, save the
/// ones it delivered, and draw offscreen the ones it didn't.
pub(crate) fn frame_end(app: &mut EditorApp, ctx: &egui::Context) {
    if core::tick(app) {
        ctx.request_repaint_after(Duration::from_millis(20));
    }
    if app.control.shots.is_empty() {
        return;
    }
    let now = Instant::now();
    for shot in &mut app.control.shots {
        shot.frames = shot.frames.saturating_add(1);
        if !shot.offscreen && shot.requested_at.is_none() && shot.frames >= SETTLE_FRAMES {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(shot.token)));
            shot.requested_at = Some(now);
        }
    }
    let images: Vec<(u64, Arc<egui::ColorImage>)> = ctx.input(|i| {
        i.raw
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Screenshot { user_data, image, .. } => Some((user_data.data.as_ref()?.downcast_ref::<u64>().copied()?, Arc::clone(image))),
                _ => None,
            })
            .collect()
    });
    for (token, image) in images {
        if let Some(i) = app.control.shots.iter().position(|s| s.token == token) {
            let shot = app.control.shots.remove(i);
            (shot.reply)(save_reply(&image, &shot.path, "window"));
        }
    }
    // The window never answered in time (a locked screen), or the caller asked: draw offscreen.
    let wait = app.control.window_shot_wait;
    let (late, keep): (Vec<Shot>, Vec<Shot>) = std::mem::take(&mut app.control.shots)
        .into_iter()
        .partition(|s| if s.offscreen { s.frames >= SETTLE_FRAMES } else { s.requested_at.is_some_and(|at| now.duration_since(at) >= wait) });
    app.control.shots = keep;
    if !late.is_empty() {
        let image = app.render_offscreen(ctx);
        for shot in late {
            let reply = match &image {
                Ok(image) => save_reply(image, &shot.path, "offscreen"),
                Err(e) => err(e.clone()),
            };
            (shot.reply)(reply);
        }
    }
    if !app.control.shots.is_empty() {
        ctx.request_repaint_after(Duration::from_millis(20));
    }
}

/// Save a picture as PNG; the reply names the file, its size and where the pixels came from
/// (`window` or `offscreen`).
pub fn save_reply(image: &egui::ColorImage, path: &Path, source: &str) -> Value {
    let [w, h] = image.size;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return err(format!("{}: {e}", dir.display()));
    }
    match crate::save_png(image, path) {
        Ok(()) => ok(json!({"path": path, "width": w, "height": h, "source": source})),
        Err(e) => err(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_checked() {
        assert!(check_fields(&json!({"zoom": 1}), &["zoom"]).is_ok());
        assert!(check_fields(&Value::Null, &["zoom"]).is_ok());
        assert!(check_fields(&json!({"zom": 1}), &["zoom"]).unwrap_err().contains("zom"));
        assert!(check_fields(&json!([1]), &["zoom"]).is_err());
    }

    #[test]
    fn png_is_written() {
        let dir = plumcraft_scratch::root().join("control-test");
        let path = dir.join("sub").join("shot.png");
        let image = egui::ColorImage::new([4, 3], vec![egui::Color32::RED; 12]);
        let r = save_reply(&image, &path, "window");
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!((r["result"]["width"].as_u64(), r["result"]["source"].as_str()), (Some(4), Some("window")));
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
