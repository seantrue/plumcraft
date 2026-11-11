//! Control-channel handlers that need the UI: `command.run`/`engine.execute`,
//! `command.list`/`engine.commands`, `graph.get`, `ui.inspect`, `ui.set`, `ui.invoke`,
//! `ui.prompt`, `ui.audio`, `ui.screenshot` and `ui.waitRun` (`docs/control-protocol.md`).
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
use serde_json::{Value, json};

use crate::EditorApp;

/// Sends one reply (`{"ok": true, "result"}` or `{"ok": false, "error"}`).
pub type Reply = Box<dyn FnOnce(Value) + Send>;

/// Frames to wait before capturing, so layout and fades settle.
const SETTLE_FRAMES: u32 = 3;

/// How long the window has to deliver a screenshot before the app draws one offscreen.
pub const WINDOW_SHOT_WAIT: Duration = Duration::from_secs(2);

/// The longest `ui.waitRun` may wait: under the transport's 30 s reply timeout.
pub const WAIT_RUN_MAX_S: f64 = 29.0;

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

/// A `ui.waitRun` waiting for the current run to finish.
struct Wait {
    deadline: Instant,
    reply: Reply,
}

/// Screenshots and run waits in flight.
pub struct ControlState {
    shots: Vec<Shot>,
    next_token: u64,
    waits: Vec<Wait>,
    /// How long a window screenshot may take before the offscreen fallback ([`WINDOW_SHOT_WAIT`]).
    pub window_shot_wait: Duration,
}

impl Default for ControlState {
    fn default() -> Self {
        ControlState { shots: Vec::new(), next_token: 0, waits: Vec::new(), window_shot_wait: WINDOW_SHOT_WAIT }
    }
}

impl ControlState {
    /// Requests still waiting for their reply.
    pub fn pending(&self) -> usize {
        self.shots.len() + self.waits.len()
    }
}

pub fn ok(result: Value) -> Value {
    json!({"ok": true, "result": result})
}

pub fn err(message: impl Into<String>) -> Value {
    json!({"ok": false, "error": message.into()})
}

/// Where a screenshot goes when the request names no path.
pub fn default_screenshot_path(token: u64) -> PathBuf {
    std::env::temp_dir().join("plumcraft-screenshots").join(format!("plumcraft-{}-{token}.png", std::process::id()))
}

fn check_fields(params: &Value, allowed: &[&str]) -> Result<(), String> {
    match params {
        Value::Null => Ok(()),
        Value::Object(o) => match o.keys().find(|k| !allowed.contains(&k.as_str())) {
            Some(k) => Err(format!("unknown field {k:?} (allowed: {})", allowed.join(", "))),
            None => Ok(()),
        },
        _ => Err("params must be a JSON object".into()),
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

    /// Handle one control request. `reply` is called exactly once, now or a few frames later.
    pub fn handle_control(&mut self, ctx: &egui::Context, method: &str, params: &Value, reply: Reply) {
        match method {
            "ui.screenshot" => {
                if let Err(e) = check_fields(params, &["path", "offscreen"]) {
                    return reply(err(e));
                }
                let offscreen = match params.get("offscreen") {
                    None | Some(Value::Null) => false,
                    Some(Value::Bool(b)) => *b,
                    Some(_) => return reply(err("offscreen must be a boolean")),
                };
                let path = match params.get("path") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(p)) if !p.trim().is_empty() => Some(PathBuf::from(p)),
                    Some(_) => return reply(err("path must be a file path")),
                };
                self.control.next_token = self.control.next_token.wrapping_add(1);
                let token = self.control.next_token;
                let path = path.unwrap_or_else(|| default_screenshot_path(token));
                self.control.shots.push(Shot { token, path, frames: 0, requested_at: None, offscreen, reply });
                ctx.request_repaint();
            }
            "ui.waitRun" => {
                if let Err(e) = check_fields(params, &["timeout"]) {
                    return reply(err(e));
                }
                let timeout = match params.get("timeout") {
                    None | Some(Value::Null) => 25.0,
                    Some(v) => match v.as_f64().filter(|t| t.is_finite() && *t >= 0.0) {
                        Some(t) => t.min(WAIT_RUN_MAX_S),
                        None => return reply(err("timeout must be a number of seconds")),
                    },
                };
                let deadline = Instant::now() + Duration::from_secs_f64(timeout);
                self.control.waits.push(Wait { deadline, reply });
                ctx.request_repaint();
            }
            _ => {
                let v = self.control_now(ctx, method, params);
                reply(v);
            }
        }
    }

    /// Handle a request that replies at once.
    pub fn control_now(&mut self, ctx: &egui::Context, method: &str, params: &Value) -> Value {
        match method {
            "command.run" | "engine.execute" => {
                if let Err(e) = check_fields(params, &["command", "params"]) {
                    return err(e);
                }
                let Some(command) = params.get("command").and_then(Value::as_str) else {
                    return err("params.command must be a command id");
                };
                let p = params.get("params").cloned().unwrap_or(Value::Null);
                let r = self.try_run(command, &p);
                ctx.request_repaint();
                match r {
                    Ok(v) => ok(v),
                    Err(e) => err(e),
                }
            }
            "command.list" | "engine.commands" => ok(plumcraft_automation::command_rows(&self.session)),
            "graph.get" => ok(serde_json::to_value(self.session.graph()).unwrap_or(Value::Null)),
            "ui.inspect" => ok(self.inspect(ctx)),
            "ui.audio" => match self.ui_audio(params) {
                Ok(()) => {
                    ctx.request_repaint();
                    ok(self.audio.inspect())
                }
                Err(e) => err(e),
            },
            "ui.invoke" => {
                if let Err(e) = check_fields(params, &["command"]) {
                    return err(e);
                }
                let Some(id) = params.get("command").and_then(Value::as_str) else { return err("command must be a command id") };
                let Some(spec) = plumcraft_session::find(id) else { return err(format!("unknown command {:?}", id.chars().take(64).collect::<String>())) };
                if let Err(why) = (spec.enabled)(&self.session) {
                    return err(format!("{id} is not available: {why}"));
                }
                self.last_error = None;
                self.invoke(spec.id);
                ctx.request_repaint();
                let mut out = self.inspect(ctx);
                out["invoked"] = json!(spec.id);
                ok(out)
            }
            "ui.prompt" => {
                if let Err(e) = check_fields(params, &["answer"]) {
                    return err(e);
                }
                let answer = match params.get("answer").and_then(Value::as_str) {
                    Some("discard") => crate::PromptAnswer::Discard,
                    Some("save") => crate::PromptAnswer::Save,
                    Some("cancel") => crate::PromptAnswer::Cancel,
                    Some("trust") => crate::PromptAnswer::Trust,
                    _ => return err("answer must be \"save\", \"discard\", \"trust\" or \"cancel\""),
                };
                if !self.answer_prompt(answer) {
                    return err("no prompt takes that answer");
                }
                ctx.request_repaint();
                ok(self.inspect(ctx))
            }
            "ui.set" => match self.ui_set(params) {
                Ok(()) => {
                    ctx.request_repaint();
                    ok(self.inspect(ctx)["canvas"].clone())
                }
                Err(e) => err(e),
            },
            other => err(format!("unknown method {other:?}")),
        }
    }

    /// The UI state as JSON.
    /// The run animation: whether the colours are on, the animation speed, whether a replay is
    /// showing, how strongly it shows, and each node
    /// of the run as shown ([`crate::run_anim`]).
    fn run_anim_inspect(&self) -> Value {
        let now = Instant::now();
        let a = &self.run_anim;
        let shown: serde_json::Map<String, Value> =
            self.session.run.needed.iter().filter_map(|id| a.shown(id, now).map(|s| (id.clone(), json!(s.name())))).collect();
        json!({"colors": !self.session.views.run_colors_off, "animMs": self.session.views.anim_ms(), "replay": a.is_replay(), "generation": a.generation(),
               "strength": a.strength(now), "shown": shown})
    }

    pub fn inspect(&self, ctx: &egui::Context) -> Value {
        let g = self.session.graph();
        let catalog = self.session.catalog();
        let nodes: Vec<Value> = g
            .nodes
            .iter()
            .map(|(id, n)| json!({"id": id, "kind": n.kind, "title": plumcraft_session::node_title(g, catalog, id), "pos": n.pos, "view": n.view}))
            .collect();
        let t = self.canvas.to_global;
        let r = self.canvas.rect;
        let popups = json!({
            "palette": self.canvas.palette.as_ref().map(|p| json!({"query": p.query, "selected": p.selected, "at": [p.screen_pos.x, p.screen_pos.y]})),
            "anyPopup": egui::Popup::is_any_open(ctx),
        });
        let run = &self.session.run;
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
        json!({
            "popouts": self.popouts_inspect(),
            "parameters": self.session.views.parameters.as_ref().map(|n| json!({"node": n, "fields": self.params.rects.iter().map(|(k, _)| k).collect::<Vec<_>>()})),
            "editors": self.value_editors.inspect(),
            "wirePreview": self.wires.inspect(),
            "title": self.window_title(),
            "document": self.document_name(),
            "path": self.session.path(),
            "dirty": self.session.is_dirty(),
            "undo": self.session.undo_label(),
            "redo": self.session.redo_label(),
            "prompt": self.prompt.as_ref().map(|p| {
                let answers = if matches!(p, crate::Prompt::Review(_) | crate::Prompt::Uploads(_)) { json!(["trust", "cancel"]) } else { json!(["save", "discard", "cancel"]) };
                json!({"kind": p.kind(), "text": p.text(), "answers": answers})
            }),
            "copied": self.copied.is_some(),
            "closing": self.closing,
            "graph": {"dirty": self.session.is_dirty(), "nodes": nodes, "wires": g.wires.len(), "generation": self.session.generation()},
            "selection": self.canvas.selection(),
            "canvas": {"zoom": t.scaling, "pan": [t.translation.x, t.translation.y],
                       "rect": if r.is_finite() { json!([r.min.x, r.min.y, r.max.x, r.max.y]) } else { Value::Null },
                       "editing": self.canvas.edits.is_editing()},
            "engine": self.engine_inspect(),
            "banner": self.banner_text(),
            "about": self.session.views.about.then_some(&self.session.about),
            "catalog": {"nodes": catalog.nodes.len(), "engine": catalog.engine.name, "version": catalog.engine.version},
            "popups": popups,
            "lastError": self.last_error,
            "run": run.summary(Instant::now()),
            "runAnim": self.run_anim_inspect(),
            "watched": watched,
            "theme": self.tokens.name,
            "prefs": {"path": self.prefs.path(), "theme": self.prefs.prefs().theme, "dirs": self.prefs.prefs().dirs},
            "groups": crate::groups_view::inspect(&self.canvas, g),
            "frames": crate::frame_view::inspect(g),
            "rename": self.rename.as_ref().map(rename_inspect),
            "docs": self.session.docs.inspect(),
            "save": self.session.save.summary(),
        })
    }

    /// `ui.audio {play?, loop?}`: play or stop the watched audio, loop it or not. The reply is
    /// the audio view's state (as `ui.inspect`'s `watched.audio`).
    fn ui_audio(&mut self, params: &Value) -> Result<(), String> {
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
    fn ui_set(&mut self, params: &Value) -> Result<(), String> {
        check_fields(params, &["zoom", "pan", "fit", "palette", "focus"])?;
        if let Some(f) = params.get("focus") {
            crate::groups_view::set_focus(&mut self.canvas, self.session.graph(), f)?;
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
    answer_waits(app, ctx);
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

/// Answer `ui.waitRun`s whose run finished (or whose time ran out).
fn answer_waits(app: &mut EditorApp, ctx: &egui::Context) {
    if app.control.waits.is_empty() {
        return;
    }
    let now = Instant::now();
    let busy = app.session.run.is_busy(now);
    let waits = std::mem::take(&mut app.control.waits);
    for w in waits {
        if !busy {
            (w.reply)(ok(app.session.run.summary(now)));
        } else if now >= w.deadline {
            (w.reply)(err("timeout: the run is still going"));
        } else {
            app.control.waits.push(w);
        }
    }
    if !app.control.waits.is_empty() {
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
