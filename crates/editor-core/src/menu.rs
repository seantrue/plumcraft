//! The menu model: every menu the editor shows, as data, for every shell (N1 slice 8,
//! `docs/shell-interface.md` section 12, `docs/research/24-native-shells.md` sections 4.1 and
//! 5.5). The egui shell draws its menu bar and its context menus from it; a macOS shell builds an
//! `NSMenu` from the same items, an iPad one `UIMenu`s and `UIKeyCommand`s, an Android one its
//! own; `menu.get` hands it to any shell over the control channel and the C ABI.
//!
//! A menu is a tree of [`Item`]s, built from the session's command registry (each command's
//! `menu` path, `label`, `shortcut` and `enabled`), the document's state, what the shell knows
//! that the core doesn't yet ([`Facts`]: the selection, the canvas's centre, the application's
//! value editors) and the platform's capabilities ([`Capabilities`]).
//!
//! - **The menu bar** ([`bar`], [`bar_menu`]): File, Edit, View, Node, Run, then each menu a
//!   command names (Group, Engine, Help), each command at its `menu` path in registry order,
//!   submenus after a menu's own items; then what is not a plain registry entry: the Edit menu's
//!   canvas items, View › Animation Speed and Fit, Node › Add and Search…, the Engine menu's
//!   engines (with the hostings this platform can't start, disabled with the reason).
//! - **Context menus** ([`context`], one per [`Context`]): a node, a collapsed group, a group's
//!   header, a wire, an input, an output, a frame, a net's tag, the canvas.
//!
//! **Invoking an item** is `ui.invoke {command}` for an item with a command and no params, and
//! `command.run {command, params}` for one with params. The Edit menu's canvas items, Fit,
//! Search… and the canvas menu are commands since N2 slice 1 ([`crate::canvas_cmds`]; the
//! selection is the core's). An item with no command is one the shell still does itself
//! (Align ›, Distribute ›, which take its measured sizes; a net's Go to Driver or Next Use,
//! which it resolves to a `canvas.reveal`; the rename and name editors); its `id` names it.
//!
//! **Roles** mark items a platform places itself ([`Role`]): a macOS shell puts About,
//! Preferences and Quit in the application menu, an iPadOS shell leaves Quit out, and the edit
//! roles bind to the system's own Undo, Cut, Copy and Paste.
//!
//! **Shortcuts** are data ([`Shortcut`]): a key and modifiers, `primary` being ⌘ on Apple
//! platforms and Ctrl elsewhere. [`shortcut_table`] is what a shell binds, most specific first.
//!
//! **Revisions** ([`Revision`]): the model's generation goes up only when what it holds changed,
//! so a native shell rebuilds its menus then and not every frame.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use plumcraft_graph::{GroupId, NodeCache, NodeId, NodeView, PortRef};
use plumcraft_proto::catalogue::{NodeDef, SchemaReply, texts};
use plumcraft_proto::event::NodeState;
use plumcraft_session::{CommandSpec, EngineChoice, EngineTarget, Session, command_specs};
use serde::Serialize;
use serde_json::{Value, json};

#[cfg(test)]
use crate::engines::offers;
use crate::engines::{Capabilities, refused};
use crate::rows::{port_label, proxy_outputs, shown_rows, unique_name};

/// The menu bar's first menus, in order; menus a command names follow.
pub const MENUS: &[&str] = &["File", "Edit", "View", "Node", "Run"];

/// A frame's colours (Frame › Colour), by name.
pub const SWATCHES: [(&str, &str); 6] =
    [("Blue", "#3d6fb6"), ("Green", "#3f8f5a"), ("Amber", "#b5852e"), ("Red", "#b54a4a"), ("Violet", "#7a58b0"), ("Grey", "#6b7280")];

/// The save buttons, on a result node and in the Watched panel.
pub const SAVE_RESULT: &str = "Save Result…";
pub const SAVE_HELP: &str = "Save this result as a file with its recipe";
pub const CONTACT_SHEET: &str = "Contact Sheet…";
pub const SHEET_HELP: &str = "Save every item on one picture, each labelled with the value that varied, with the sweep's recipe";

/// The Edit menu's Align › edges, as `edit.align`'s `edge`.
pub const ALIGN_EDGES: [(&str, &str); 6] = [
    ("Left Edges", "left"),
    ("Right Edges", "right"),
    ("Top Edges", "top"),
    ("Bottom Edges", "bottom"),
    ("Centres Horizontally", "centerX"),
    ("Centres Vertically", "centerY"),
];

/// A modifier of a [`Shortcut`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mod {
    /// ⌘ on macOS and iPadOS, Ctrl on Android, Linux and Windows (the registry's `Cmd`).
    Primary,
    /// Control everywhere (⌃ on Apple platforms).
    Ctrl,
    Shift,
    /// ⌥ on Apple platforms (the registry's `Alt` or `Opt`).
    Alt,
}

/// A keyboard shortcut as data: a key by its name (letters, digits, `Enter`, `Comma`,
/// `Period`, `Escape`, `Tab`, `Space`, `Delete`, `Backspace`, arrows, `F1`…) and modifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Shortcut {
    pub key: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mods: Vec<Mod>,
}

impl Shortcut {
    fn key(key: &str) -> Shortcut {
        Shortcut { key: key.to_owned(), mods: Vec::new() }
    }

    fn primary(key: &str) -> Shortcut {
        Shortcut { key: key.to_owned(), mods: vec![Mod::Primary] }
    }

    /// How many modifiers besides the primary one it names (Shift, Alt): a shortcut naming
    /// more is more specific, and is matched first.
    pub fn specificity(&self) -> u8 {
        self.mods.iter().filter(|m| matches!(m, Mod::Shift | Mod::Alt)).count().try_into().unwrap_or(u8::MAX)
    }
}

/// The keys a shortcut may name.
const NAMED_KEYS: &[&str] = &[
    "Enter",
    "Escape",
    "Tab",
    "Space",
    "Backspace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "Comma",
    "Period",
    "Minus",
    "Plus",
    "Equals",
    "Colon",
    "Semicolon",
    "Slash",
    "Backslash",
    "Questionmark",
    "Quote",
    "Backtick",
    "OpenBracket",
    "CloseBracket",
    "OpenCurlyBracket",
    "CloseCurlyBracket",
    "Pipe",
    "Exclamationmark",
    "Copy",
    "Cut",
    "Paste",
];

/// Is `name` a key a shortcut may name?
pub fn is_key(name: &str) -> bool {
    let one = |c: char| c.is_ascii_uppercase() || c.is_ascii_digit();
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => one(c),
        _ => NAMED_KEYS.contains(&name) || name.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=35).contains(&n)),
    }
}

/// Parse a shortcut in the registry's notation (`Cmd+Shift+Z`): `Cmd` is the primary modifier
/// (⌘ on Apple platforms, Ctrl elsewhere), `Ctrl`, `Shift`, `Alt` or `Opt`, then one key.
pub fn parse_shortcut(s: &str) -> Option<Shortcut> {
    let mut mods = Vec::new();
    let mut key = None;
    for part in s.split('+') {
        let m = match part {
            "Cmd" => Some(Mod::Primary),
            "Ctrl" => Some(Mod::Ctrl),
            "Shift" => Some(Mod::Shift),
            "Alt" | "Opt" => Some(Mod::Alt),
            k if key.is_none() && is_key(k) => {
                key = Some(k.to_owned());
                None
            }
            _ => return None,
        };
        if let Some(m) = m
            && !mods.contains(&m)
        {
            mods.push(m);
        }
    }
    mods.sort();
    Some(Shortcut { key: key?, mods })
}

/// A shortcut a shell binds: the command it runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub command: &'static str,
    pub shortcut: Shortcut,
    /// A focused text field keeps these keys for itself (its own undo).
    pub text_field_keeps: bool,
}

/// Every registry shortcut, the most specific first (a toolkit that matches Shift and Alt as
/// wildcards when a shortcut doesn't name them then finds Redo before Undo, Save As before Save).
/// The rules a shell keeps: a disabled command still takes its keys (so a disabled Redo's keys
/// don't fall through to Undo); keys fired in a text field run after the field has committed.
pub fn shortcut_table() -> Vec<Binding> {
    let mut out: Vec<Binding> = command_specs()
        .iter()
        .filter_map(|c| Some(Binding { command: c.id, shortcut: c.shortcut.and_then(parse_shortcut)?, text_field_keeps: c.id.starts_with("edit.") }))
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.shortcut.specificity()));
    out
}

/// What a platform places itself (`docs/shell-interface.md` 12; plan 5.5's conventions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    /// The application menu's About (macOS); Help elsewhere.
    About,
    /// Preferences / Settings: the application menu on macOS, ⌘, everywhere on Apple platforms.
    Preferences,
    /// Quit: the application menu on macOS; none on iPadOS and iOS.
    Quit,
    New,
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    SelectAll,
    /// A whole menu the platform knows: it adds its own items to it (macOS's Edit gets
    /// dictation and emoji, View gets full screen, Help gets search).
    FileMenu,
    EditMenu,
    ViewMenu,
    HelpMenu,
    /// Reserved: the platform's Window menu and full screen. The model has none today; a shell
    /// adds its own.
    WindowMenu,
    FullScreen,
}

/// How a toolkit that draws its own menus shows an item. A native menu shows every one of these
/// as a plain item with a check mark where `checked` is set; `Row` and `Section` hold their items
/// inline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Look {
    /// A button.
    #[default]
    Plain,
    /// A toggle with a check mark when on (`checked`).
    Check,
    /// One of a choice of several (`checked` the one chosen).
    Radio,
    /// A selectable label among others in a row.
    Choice,
    /// A selectable button (a colour swatch: `color`).
    Selected,
    /// A quiet line that does nothing (a node's id and kind).
    Note,
    /// A line of text that does nothing.
    Text,
    /// A dim line of text that does nothing.
    Dim,
    /// A label and its items, side by side.
    Row,
    /// Its items inline, in a list that scrolls when long.
    Section,
}

impl Look {
    fn is_plain(&self) -> bool {
        *self == Look::Plain
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One menu item, a submenu (it has `items`, or `submenu` when it is empty), or a separator.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// What the item is: its command's id, or (for an item the shell does itself) the command
    /// it will become; a submenu's path. Empty for a separator.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// The command activating it runs: `ui.invoke {command}`, or `command.run {command,
    /// params}` when it has `params`. None: the shell does it (its `id` says what).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    pub enabled: bool,
    /// Why it is disabled (the disabled item's tooltip).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<Shortcut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    /// The enabled item's tooltip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Item>,
    /// A submenu, even when it has no items.
    #[serde(skip_serializing_if = "is_false")]
    pub submenu: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub separator: bool,
    #[serde(skip_serializing_if = "Look::is_plain")]
    pub look: Look,
    /// Its label's colour, `#rrggbb` (a frame's swatch).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Drawn strong (an engine in use).
    #[serde(skip_serializing_if = "is_false")]
    pub strong: bool,
    /// An icon for a toolbar's button (N4 slice 3): a Lucide name from the core's set
    /// (`icon.get`), which a platform may show as its own symbol for the same meaning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl Item {
    /// A plain item running `command` (no command: the shell does it).
    pub fn new(id: &str, label: impl Into<String>, command: Option<&str>, params: Option<Value>) -> Item {
        Item { id: id.to_owned(), label: label.into(), command: command.map(str::to_owned), params, enabled: true, ..Item::default() }
    }

    /// An item running `command` with `params`.
    pub fn run(command: &str, label: impl Into<String>, params: Value) -> Item {
        Item::new(command, label, Some(command), Some(params))
    }

    /// An item the shell does itself, named `id`.
    pub fn local(id: &str, label: impl Into<String>, params: Option<Value>) -> Item {
        Item::new(id, label, None, params)
    }

    pub fn separator() -> Item {
        Item { separator: true, enabled: true, ..Item::default() }
    }

    pub fn submenu(id: &str, label: impl Into<String>, items: Vec<Item>) -> Item {
        Item { id: id.to_owned(), label: label.into(), items, submenu: true, enabled: true, ..Item::default() }
    }

    fn line(look: Look, label: impl Into<String>) -> Item {
        Item { label: label.into(), look, enabled: true, ..Item::default() }
    }

    pub fn hover(mut self, hover: impl Into<String>) -> Item {
        self.hover = Some(hover.into());
        self
    }

    fn look(mut self, look: Look, checked: bool) -> Item {
        self.look = look;
        self.checked = Some(checked);
        self
    }

    fn disabled(mut self, reason: Option<String>) -> Item {
        self.enabled = false;
        self.reason = reason;
        self
    }

    fn role(mut self, role: Role) -> Item {
        self.role = Some(role);
        self
    }

    /// Is it something to click (not a separator, a line of text, a row or a section)?
    pub fn is_action(&self) -> bool {
        !self.separator && !self.submenu && !matches!(self.look, Look::Note | Look::Text | Look::Dim | Look::Row | Look::Section)
    }

    /// This item and every item under it, depth first.
    pub fn walk(&self) -> Vec<&Item> {
        let mut out = vec![self];
        for i in &self.items {
            out.extend(i.walk());
        }
        out
    }
}

/// What the menus are built from beside the session: the selection (the document's since N2
/// slice 1), and what only the shell knows (the canvas's centre, the application's editors).
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// The selected nodes, and the selected collapsed groups.
    pub selected: Vec<NodeId>,
    pub selected_groups: Vec<GroupId>,
    /// The canvas's centre, in graph coordinates: where Node › Add puts a node.
    pub center: Option<[f32; 2]>,
    /// An engine link is up (an engine in use shows as such in the Engine menu).
    pub live: bool,
    /// What the platform can start: the Engine menu lists what it can't, disabled.
    pub caps: Capabilities,
    /// The application's value editors: `(editor.kind, label, help)` (`node.editor`).
    pub editors: Vec<(String, String, String)>,
    /// Select mode is on (`canvas.selectMode`, N4 slice 3): View › Select Mode's check mark.
    pub select_mode: bool,
}

/// The menu bar's titles, in order.
pub fn bar_titles() -> Vec<&'static str> {
    let mut menus: Vec<&'static str> = MENUS.to_vec();
    for spec in command_specs() {
        if let Some(m) = spec.menu.first()
            && !menus.contains(m)
        {
            menus.push(m);
        }
    }
    menus
}

/// The whole menu bar: one submenu per title.
pub fn bar(session: &Session, facts: &Facts) -> Vec<Item> {
    bar_titles().into_iter().map(|t| bar_menu(session, facts, t)).collect()
}

fn menu_role(title: &str) -> Option<Role> {
    match title {
        "File" => Some(Role::FileMenu),
        "Edit" => Some(Role::EditMenu),
        "View" => Some(Role::ViewMenu),
        "Help" => Some(Role::HelpMenu),
        _ => None,
    }
}

fn command_role(id: &str) -> Option<Role> {
    Some(match id {
        "app.about" => Role::About,
        "app.settings" => Role::Preferences,
        "app.quit" => Role::Quit,
        "graph.new" => Role::New,
        "graph.open" => Role::Open,
        "graph.save" => Role::Save,
        "graph.saveAs" => Role::SaveAs,
        "edit.undo" => Role::Undo,
        "edit.redo" => Role::Redo,
        _ => return None,
    })
}

/// One menu of the bar, `title`, as a submenu item.
pub fn bar_menu(session: &Session, facts: &Facts, title: &str) -> Item {
    let specs: Vec<&CommandSpec> = command_specs().iter().filter(|c| c.menu.first() == Some(&title)).collect();
    let mut items = command_items(session, &specs, 1, title);
    match title {
        "Engine" => {
            items.push(Item::separator());
            items.extend(engine_items(session, facts));
        }
        "Edit" => items.extend(edit_items(facts)),
        "View" => items.extend(view_items(session, facts)),
        "File" => {
            // Share… (N4 slice 3) after Save As
            let at = items.iter().position(|i| i.command.as_deref() == Some("graph.saveAs")).map_or(items.len(), |k| k + 1);
            items.insert(at, share_item(session));
        }
        "Node" => items.extend(node_items(session, facts)),
        "Run" => {
            // Run Selected (a canvas command: it runs the selection) after Run
            let at = items.iter().position(|i| i.command.as_deref() == Some("run.start")).map_or(items.len(), |k| k + 1);
            items.insert(at, run_selected_item(session, facts));
        }
        _ => {}
    }
    let mut m = Item::submenu(&format!("menu:{title}"), title, items);
    m.role = menu_role(title);
    m
}

/// A toggle command's state, for its check mark.
pub fn checked(session: &Session, id: &str) -> Option<bool> {
    match id {
        "run.auto" => Some(session.run.auto),
        "view.runColors" => Some(!session.views.run_colors_off),
        _ => None,
    }
}

/// The registry's commands at `depth` of a menu path (submenus for longer paths, after the
/// menu's own items, each in the order first named).
fn command_items(session: &Session, specs: &[&CommandSpec], depth: usize, path: &str) -> Vec<Item> {
    let mut items = Vec::new();
    let mut subs: Vec<&str> = Vec::new();
    for spec in specs {
        match spec.menu.get(depth) {
            None => items.push(command_item(session, spec)),
            Some(sub) if !subs.contains(sub) => subs.push(sub),
            Some(_) => {}
        }
    }
    for sub in subs {
        let inner: Vec<&CommandSpec> = specs.iter().copied().filter(|c| c.menu.get(depth) == Some(&sub)).collect();
        let at = format!("{path}/{sub}");
        items.push(Item::submenu(&format!("menu:{at}"), sub, command_items(session, &inner, depth + 1, &at)));
    }
    items
}

/// A registry command's item: its label, shortcut, enabled state, check mark and role.
pub fn command_item(session: &Session, spec: &CommandSpec) -> Item {
    let mut item = Item::new(spec.id, spec.label, Some(spec.id), None);
    if let Err(why) = (spec.enabled)(session) {
        item = item.disabled(Some(why));
    }
    if let Some(on) = checked(session, spec.id) {
        item.look = Look::Check;
        item.checked = Some(on);
    }
    item.shortcut = spec.shortcut.and_then(parse_shortcut);
    item.role = command_role(spec.id);
    item
}

/// The Edit menu's canvas items: commands that `ui.invoke` fills with the selection (N2 slice
/// 1), but Align and Distribute, which take the sizes the shell measured and so stay the shell's
/// until it reports its geometry (slice 3).
fn edit_items(facts: &Facts) -> Vec<Item> {
    let many = facts.selected.len() + facts.selected_groups.len();
    let has = many > 0;
    let item = |id: &str, label: &str, key: Option<Shortcut>, enabled: bool, role: Option<Role>| {
        let mut i = Item::new(id, label, Some(id), None);
        i.shortcut = key;
        i.role = role;
        if enabled { i } else { i.disabled(None) }
    };
    let mut items = vec![
        Item::separator(),
        item("edit.cut", "Cut", Some(Shortcut::primary("X")), has, Some(Role::Cut)),
        item("edit.copy", "Copy", Some(Shortcut::primary("C")), has, Some(Role::Copy)),
        item("edit.paste", "Paste", Some(Shortcut::primary("V")), true, Some(Role::Paste)),
        item("edit.delete", "Delete", Some(Shortcut::key("Backspace")), has, Some(Role::Delete)),
        item("selection.all", "Select All", Some(Shortcut::primary("A")), true, Some(Role::SelectAll)),
        item("frame.add", "Frame Selection", None, has, None),
        Item::separator(),
    ];
    // Align and Distribute: the core's since N2 slice 3, at the sizes the shell measured
    let align = ALIGN_EDGES.iter().map(|(label, edge)| Item::new("edit.align", *label, Some("selection.align"), Some(json!({"edge": edge})))).collect();
    let mut m = Item::submenu("edit.align", "Align", align);
    if many < 2 {
        m = m.disabled(None);
    }
    items.push(m);
    let distribute = [("Horizontally", "horizontal"), ("Vertically", "vertical")]
        .iter()
        .map(|(label, axis)| Item::new("edit.distribute", *label, Some("selection.distribute"), Some(json!({"axis": axis}))))
        .collect();
    let mut m = Item::submenu("edit.distribute", "Distribute", distribute);
    if many < 3 {
        m = m.disabled(None);
    }
    items.push(m);
    items
}

/// Run › Run Selected: enabled by the selection, with the reason when it can't run.
fn run_selected_item(session: &Session, facts: &Facts) -> Item {
    let item = Item::new("run.selected", "Run Selected", Some("run.selected"), None).hover("Run the selected nodes and what they need upstream");
    match crate::canvas_cmds::run_selected_reason(session, facts.selected.len() + facts.selected_groups.len()) {
        Some(why) => item.disabled(Some(why)),
        None => item,
    }
}

/// File › Share…: the graph's file to the platform's share sheet, once it has one.
fn share_item(session: &Session) -> Item {
    let item = Item::new("graph.share", "Share…", Some("graph.share"), None).hover("Send the graph's file with the platform's share sheet");
    match session.path() {
        Some(_) => item,
        None => item.disabled(Some("save the graph first: Share sends its file".into())),
    }
}

/// View › Select Mode: a one-finger drag on empty canvas draws a marquee (touch).
fn select_mode_item(facts: &Facts) -> Item {
    Item::new("canvas.selectMode", "Select Mode", Some("canvas.selectMode"), None)
        .hover("A one-finger drag on empty canvas draws a marquee rather than panning (touch)")
        .look(Look::Check, facts.select_mode)
}

/// The toolbar (N4 slice 3; `menu.get {context: {kind: "toolbar"}}`): every button a shell's
/// toolbar shows, in order, with its label, icon, enabled state, reason and check mark from the
/// core, so no shell keeps a button's state of its own. A separator divides groups.
pub fn toolbar(session: &Session, facts: &Facts) -> Vec<Item> {
    let icon = |mut i: Item, name: &str| {
        i.icon = Some(name.to_owned());
        i
    };
    let mut add = Item::new("canvas.palette", "Add Node", Some("canvas.palette"), None).hover("Add a node from the palette (Tab)");
    add.shortcut = Some(Shortcut::key("Tab"));
    let mut fit = Item::new("canvas.fit", "Fit", Some("canvas.fit"), None).hover("Fit the view to all nodes (F)");
    fit.shortcut = Some(Shortcut::key("F"));
    let run = command_specs()
        .iter()
        .find(|c| c.id == "run.start")
        .map(|c| command_item(session, c))
        .unwrap_or_else(|| Item::new("run.start", "Run", Some("run.start"), None));
    let cancel = command_specs()
        .iter()
        .find(|c| c.id == "run.cancel")
        .map(|c| command_item(session, c))
        .unwrap_or_else(|| Item::new("run.cancel", "Cancel", Some("run.cancel"), None));
    let mut run = run.hover("Run the graph");
    run.label = "Run".into();
    let mut cancel = cancel.hover("Cancel the run");
    cancel.label = "Cancel".into();
    let mut select = select_mode_item(facts);
    select.label = "Select".into();
    let mut share = share_item(session);
    share.label = "Share".into();
    vec![
        icon(add, "plus"),
        icon(fit, "scan"),
        Item::separator(),
        icon(run, "play"),
        icon(run_selected_item(session, facts), "square-play"),
        icon(cancel, "square"),
        Item::separator(),
        icon(select, "square-dashed-mouse-pointer"),
        icon(share, "share"),
    ]
}

/// View › Animation Speed, Fit and Select Mode.
fn view_items(session: &Session, facts: &Facts) -> Vec<Item> {
    let speeds = crate::run_anim::SPEEDS
        .iter()
        .map(|&ms| {
            let (label, hover) = if ms == 0 {
                ("Real Time".to_owned(), "Replay every node at its recorded timing")
            } else {
                (format!("{ms} ms per node"), "Show each node running at least this long")
            };
            Item::run("view.animationSpeed", label, json!({"ms": ms})).hover(hover).look(Look::Check, session.views.anim_ms() == ms)
        })
        .collect();
    let mut fit = Item::new("canvas.fit", "Fit", Some("canvas.fit"), None).hover("Fit the view to all nodes");
    fit.shortcut = Some(Shortcut::key("F"));
    vec![Item::submenu("view.animationSpeed", "Animation Speed", speeds), fit, select_mode_item(facts)]
}

/// Node › Add (by category, under a menu per engine with several) and Search….
fn node_items(session: &Session, facts: &Facts) -> Vec<Item> {
    let catalog = session.catalog();
    let add =
        if catalog.nodes.is_empty() { vec![Item::line(Look::Text, "No node types: the engine is not connected.")] } else { add_tree(catalog, facts.center) };
    let mut search = Item::new("canvas.palette", "Search…", Some("canvas.palette"), None);
    search.shortcut = Some(Shortcut::key("Tab"));
    // Node › Edit Parameters… (N4 slice 3): the inspector on the one node selected, ⌘I
    let mut params = match facts.selected.as_slice() {
        [one] if facts.selected_groups.is_empty() => Item::run("node.parameters", "Edit Parameters…", json!({"node": one})),
        _ => Item::new("node.parameters", "Edit Parameters…", Some("node.parameters"), None).disabled(Some("select one node to edit its parameters".into())),
    };
    params.shortcut = Some(Shortcut::primary("I"));
    params.hover = Some("Every input of the node, shown on its face or not".into());
    vec![Item::submenu("node.add", "Add", add), search, params]
}

/// Nested menus of node types by category path, under a menu per engine first when several
/// are connected (the kinds more than one offers under Shared).
fn add_tree(catalog: &SchemaReply, center: Option<[f32; 2]>) -> Vec<Item> {
    #[derive(Default)]
    struct Tree<'a> {
        children: BTreeMap<&'a str, Tree<'a>>,
        leaves: Vec<&'a NodeDef>,
    }
    fn items(tree: &Tree<'_>, center: Option<[f32; 2]>, path: &str) -> Vec<Item> {
        let mut out = Vec::new();
        for (name, sub) in &tree.children {
            let at = format!("{path}/{name}");
            out.push(Item::submenu(&format!("node.add:{at}"), *name, items(sub, center, &at)));
        }
        for def in &tree.leaves {
            let params = match center {
                Some(c) => json!({"kind": def.id, "pos": [c[0], c[1]]}),
                None => json!({"kind": def.id}),
            };
            let mut item = Item::run("node.add", crate::palette::display_name(def), params);
            if !def.description.is_empty() {
                item.hover = Some(def.description.clone());
            }
            out.push(item);
        }
        out
    }
    fn sort_leaves(t: &mut Tree<'_>) {
        t.leaves.sort_by_key(|d| crate::palette::display_name(d).to_lowercase());
        for c in t.children.values_mut() {
            sort_leaves(c);
        }
    }
    let several = catalog.nodes.iter().flat_map(|d| d.engines.iter()).collect::<std::collections::BTreeSet<_>>().len() > 1;
    let mut root = Tree::default();
    for def in catalog.nodes.iter().filter(|d| crate::palette::offered(d)) {
        let mut at = &mut root;
        if several {
            let branch = match def.engines.as_slice() {
                [one] => one.as_str(),
                [] => "Other",
                _ => "Shared",
            };
            at = at.children.entry(branch).or_default();
        }
        if def.category.is_empty() {
            at = at.children.entry("Other").or_default();
        }
        for part in &def.category {
            at = at.children.entry(part.as_str()).or_default();
        }
        at.leaves.push(def);
    }
    sort_leaves(&mut root);
    items(&root, center, "")
}

/// The Engine menu's engines, after its commands: Remove for each added engine, then every
/// engine (a submenu of its hostings, the one in use checked; picking automatically, one item
/// that starts it the fastest way). A hosting this platform can't start is listed, disabled,
/// with the reason.
fn engine_items(session: &Session, facts: &Facts) -> Vec<Item> {
    let engines = &session.engines;
    if engines.choices.is_empty() {
        return vec![Item::line(Look::Dim, "This editor can't start engines.")];
    }
    let mut out = Vec::new();
    let mut removable = false;
    for t in &engines.added {
        if let EngineTarget::Choice { engine, .. } = t {
            removable = true;
            let label = format!("Remove {}", crate::switchboard::target_label(session, t));
            out.push(Item::run("engine.remove", label, json!({"engine": engine})).hover(format!("engine.remove {{engine: {engine:?}}}")));
        }
    }
    if removable {
        out.push(Item::separator());
    }
    // what can start, then (where the platform lacks something) what can't, by engine
    let mut all: Vec<(&EngineChoice, Option<String>)> = engines.choices.iter().map(|c| (c, None)).collect();
    let refused = refused_cached(&facts.caps);
    for o in &refused {
        if !all.iter().any(|(c, _)| c.engine == o.choice.engine && c.mode == o.choice.mode) {
            all.push((&o.choice, o.why_not.clone()));
        }
    }
    type Hostings<'a> = Vec<(&'a EngineChoice, Option<String>)>;
    let mut groups: Vec<(&str, Hostings<'_>)> = Vec::new();
    for (c, why) in all {
        match groups.iter_mut().find(|(_, g)| g.first().is_some_and(|(f, _)| f.engine == c.engine)) {
            Some((_, g)) => g.push((c, why)),
            None => groups.push((c.engine_label.as_str(), vec![(c, why)])),
        }
    }
    let mut list = Vec::new();
    for (label, modes) in groups {
        let engine = modes.first().map_or("", |(c, _)| c.engine.as_str());
        let using = engines.in_use(engine).filter(|_| facts.live).and_then(|m| modes.iter().find(|(c, why)| c.mode == m && why.is_none()).map(|(c, _)| *c));
        let heading = match using {
            Some(c) => format!("{label} · {}", c.mode_label),
            None => label.to_owned(),
        };
        if engines.auto {
            let startable = modes.iter().any(|(_, why)| why.is_none());
            let mut item = Item::run("engine.use", heading, json!({"engine": engine}))
                .hover(format!("engine.use {{engine: {engine:?}}}"))
                .look(Look::Radio, using.is_some());
            item.strong = using.is_some();
            if !startable {
                item = item.disabled(modes.first().and_then(|(_, why)| why.clone()));
            }
            list.push(item);
            continue;
        }
        let hostings = modes
            .iter()
            .map(|(c, why)| {
                let on = using.is_some_and(|u| u.mode == c.mode);
                let item = Item::run("engine.use", c.mode_label.clone(), json!({"engine": c.engine, "mode": c.mode}))
                    .hover(format!("engine.use {{engine: {:?}, mode: {:?}}}", c.engine, c.mode))
                    .look(Look::Radio, on);
                if why.is_some() { item.disabled(why.clone()) } else { item }
            })
            .collect();
        let mut sub = Item::submenu(&format!("engine:{engine}"), heading, hostings);
        sub.strong = using.is_some();
        list.push(sub);
    }
    let mut section = Item::submenu("engine.choices", "", list);
    section.submenu = false;
    section.look = Look::Section;
    out.push(section);
    out
}

/// [`refused`], worked out once per platform and list of engines on each thread: a craft's
/// label reads its manifest, which a menu built on every wake shouldn't.
fn refused_cached(caps: &Capabilities) -> Vec<crate::engines::Offer> {
    type Cache = Option<(Capabilities, usize, Vec<crate::engines::Offer>)>;
    thread_local! {
        static CACHE: std::cell::RefCell<Cache> = const { std::cell::RefCell::new(None) };
    }
    let kinds = crate::engines::EngineKind::all().len();
    CACHE.with_borrow_mut(|c| match c {
        Some((k, n, offers)) if k == caps && *n == kinds => offers.clone(),
        _ => {
            let offers = refused(caps);
            *c = Some((*caps, kinds, offers.clone()));
            offers
        }
    })
}

/// What a context menu is for. The shell passes what it hit.
#[derive(Debug, Clone, PartialEq)]
pub enum Context {
    /// A node (not a collapsed group).
    Node { node: NodeId },
    /// A collapsed group's node.
    Proxy { group: GroupId },
    /// An open group's header.
    Group { group: GroupId },
    /// A wire, by the input it goes into. `bend`: where Add Bend Point would put the point, if the
    /// shell knows (the wire's route with the point added).
    Wire { to: PortRef, bend: Option<Value> },
    /// An input's caption.
    Input { port: PortRef },
    /// An output's caption.
    Output { port: PortRef },
    /// A frame's title.
    Frame { frame: String },
    /// A net's tag: the input it goes into (a use), else the net's driving tag.
    Net { net: plumcraft_graph::NetKey, to: Option<PortRef> },
    /// The empty canvas.
    Canvas,
    /// The toolbar's buttons (N4 slice 3, [`toolbar`]).
    Toolbar,
}

impl Context {
    /// A context from `menu.get`'s `context`: `{kind, …}`. A hostile shape is an error.
    pub fn parse(v: &Value) -> Result<Context, String> {
        let Some(o) = v.as_object() else { return Err("context must be {kind, …}".into()) };
        let kind = o.get("kind").and_then(Value::as_str).ok_or("context.kind must be a string")?;
        let id = |k: &str| -> Result<String, String> {
            o.get(k).and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 256).map(str::to_owned).ok_or_else(|| format!("context.{k} must be an id"))
        };
        let port = |k: &str| -> Result<PortRef, String> {
            let p = o.get(k).ok_or_else(|| format!("context.{k} must be {{node, port}}"))?;
            let node = p.get("node").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 256);
            let name = p.get("port").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 256);
            match (node, name) {
                (Some(n), Some(pn)) => Ok(PortRef::new(n, pn)),
                _ => Err(format!("context.{k} must be {{node, port}}")),
            }
        };
        Ok(match kind {
            "node" => Context::Node { node: NodeId::from(id("node")?.as_str()) },
            "proxy" => Context::Proxy { group: GroupId::from(id("group")?.as_str()) },
            "group" => Context::Group { group: GroupId::from(id("group")?.as_str()) },
            "wire" => {
                // where the press was (N4 slice 3): Add Bend Point puts the point there
                let bend = match o.get("at") {
                    None | Some(Value::Null) => None,
                    Some(v) => {
                        let a = v.as_array().filter(|a| a.len() == 2).ok_or("context.at must be [x, y]")?;
                        let c = |v: &Value| v.as_f64().filter(|f| f.is_finite() && f.abs() <= 1.0e7).ok_or("context.at must be [x, y] numbers");
                        Some(json!({"at": [c(&a[0])?, c(&a[1])?]}))
                    }
                };
                Context::Wire { to: port("to")?, bend }
            }
            "input" => Context::Input { port: port("port")? },
            "output" => Context::Output { port: port("port")? },
            "frame" => Context::Frame { frame: id("frame")? },
            "net" => {
                let name = id("net")?;
                let group = match o.get("group") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(g)) if !g.is_empty() && g.len() <= 256 => Some(GroupId::from(g.as_str())),
                    Some(_) => return Err("context.group must be a group id".into()),
                };
                let to = if o.contains_key("to") { Some(port("to")?) } else { None };
                Context::Net { net: plumcraft_graph::NetKey { name, group }, to }
            }
            "canvas" => Context::Canvas,
            "toolbar" => Context::Toolbar,
            other => {
                return Err(format!(
                    "context.kind {:?}: one of node, proxy, group, wire, input, output, frame, net, canvas, toolbar",
                    other.chars().take(32).collect::<String>()
                ));
            }
        })
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Context::Node { .. } => "node",
            Context::Proxy { .. } => "proxy",
            Context::Group { .. } => "group",
            Context::Wire { .. } => "wire",
            Context::Input { .. } => "input",
            Context::Output { .. } => "output",
            Context::Frame { .. } => "frame",
            Context::Net { .. } => "net",
            Context::Canvas => "canvas",
            Context::Toolbar => "toolbar",
        }
    }
}

/// What a context menu reads of a document: its graph, catalogue, runs and the apps that open
/// its values. A shell that draws the canvas lends these without the whole session.
#[derive(Clone, Copy)]
pub struct Parts<'a> {
    pub graph: &'a plumcraft_graph::Graph,
    pub catalog: &'a SchemaReply,
    pub run: &'a plumcraft_session::RunState,
    pub doc_apps: &'a [plumcraft_session::DocApp],
    pub recent_apps: &'a [std::path::PathBuf],
}

impl<'a> Parts<'a> {
    pub fn of(session: &'a Session) -> Parts<'a> {
        Parts { graph: session.graph(), catalog: session.catalog(), run: &session.run, doc_apps: &session.docs.apps, recent_apps: &session.docs.recent_apps }
    }
}

/// The context menu for `cx`: empty when what it names isn't there.
pub fn context(parts: &Parts<'_>, facts: &Facts, cx: &Context) -> Vec<Item> {
    match cx {
        Context::Node { node } => node_menu(parts, facts, node),
        Context::Proxy { group } => proxy_menu(parts, group),
        Context::Group { group } => group_menu(parts.graph, group),
        Context::Wire { to, bend } => wire_menu(parts, to, bend.as_ref()),
        Context::Input { port } => input_menu(parts, port),
        Context::Output { port } => vec![Item::run("run.watch", format!("Watch {}", port.port), json!({"node": port.node, "port": port.port}))],
        Context::Frame { frame } => frame_menu(parts.graph, frame),
        Context::Net { net, to } => net_menu(parts.graph, net, to.as_ref()),
        Context::Canvas => canvas_menu(facts),
        // the toolbar needs the session's commands' states: `get` builds it
        Context::Toolbar => Vec::new(),
    }
}

/// The canvas's own menu. Today a right-click on the canvas opens the palette, so the egui shell
/// draws none; a touch shell shows these on a long press.
fn canvas_menu(facts: &Facts) -> Vec<Item> {
    vec![
        Item::new("canvas.palette", "Add Node…", Some("canvas.palette"), None),
        Item::new("edit.paste", "Paste", Some("edit.paste"), None).role(Role::Paste),
        Item::new("selection.all", "Select All", Some("selection.all"), None).role(Role::SelectAll),
        Item::new("canvas.fit", "Fit", Some("canvas.fit"), None),
        select_mode_item(facts),
    ]
}

/// A node's menu (the right-click menu, and the menu in its header).
fn node_menu(parts: &Parts<'_>, facts: &Facts, id: &NodeId) -> Vec<Item> {
    let graph = parts.graph;
    let catalog = parts.catalog;
    let Some(node) = graph.node(id) else { return Vec::new() };
    let def = catalog.node(&node.kind);
    let mut out = vec![Item::run("node.parameters", "Edit Parameters…", json!({"node": id})).hover("Every input of this node in a window of its own")];
    // the application's editor, while the value it edits isn't on a wire
    if let Some(d) = def
        && let Some(ed) = &d.editor
        && let Some((_, label, help)) = facts.editors.iter().find(|(k, ..)| k == &ed.kind)
        && graph.wire_into(&PortRef::new(id.clone(), ed.port.clone())).is_none()
    {
        out.push(Item::run("node.editor", label.clone(), json!({"node": id})).hover(help.clone()));
    }
    out.push(Item::line(Look::Note, format!("{} ({})", id, node.kind)));
    let shown = plumcraft_session::node_title(graph, catalog, id);
    out.push(Item::run("name.edit", "Rename…", json!({"node": id})).hover(format!("Title this node (now \"{shown}\")")));
    // how it shows, what it keeps, where it runs: for the selection when the node is in it
    let targets: Vec<NodeId> = if facts.selected.contains(id) { facts.selected.clone() } else { vec![id.clone()] };
    let row = |label: &str, items: Vec<Item>| Item { label: label.to_owned(), items, look: Look::Row, enabled: true, ..Item::default() };
    let views = [
        (NodeView::Full, "Full", "full", "Pins, widgets and the preview"),
        (NodeView::Compact, "No preview", "compact", "Pins and widgets, without the preview"),
        (NodeView::Collapsed, "Collapsed", "collapsed", "The title only"),
    ];
    out.push(row(
        "View",
        views
            .iter()
            .map(|(v, name, key, tip)| Item::run("node.view", *name, json!({"nodes": targets, "view": key})).hover(*tip).look(Look::Choice, node.view == *v))
            .collect(),
    ));
    let caches = [
        (NodeCache::Auto, "When slow", "auto", "Kept in the store across restarts when the node took a second or more"),
        (NodeCache::Always, "Always", "always", "Always kept in the store across restarts"),
        (NodeCache::Never, "Never", "never", "Computed again after a restart"),
    ];
    out.push(row(
        "Keep result",
        caches
            .iter()
            .map(|(c, name, key, tip)| Item::run("node.cache", *name, json!({"nodes": targets, "cache": key})).hover(*tip).look(Look::Choice, node.cache == *c))
            .collect(),
    ));
    // with several engines offering the kind: pin the nodes to one
    if let Some(offered) = def.map(|d| d.engines.clone()).filter(|e| e.len() > 1) {
        let pin = node.engine.clone().filter(|p| offered.contains(p));
        let engine =
            pin.clone().or_else(|| graph.engines.iter().find(|e| offered.contains(e)).cloned()).or_else(|| offered.first().cloned()).unwrap_or_default();
        let mut items: Vec<Item> = offered
            .iter()
            .map(|e| {
                Item::run("node.engine", e.clone(), json!({"nodes": targets, "engine": e}))
                    .hover(format!("node.engine {{engine: {e:?}}}"))
                    .look(Look::Choice, &engine == e && pin.is_some())
            })
            .collect();
        items.push(
            Item::run("node.engine", "Any", json!({"nodes": targets, "engine": null}))
                .hover("Unpinned: the first of the workflow's engines that offers it")
                .look(Look::Choice, pin.is_none()),
        );
        out.push(row("Run on", items));
    }
    out.push(Item::separator());
    let outputs: Vec<(String, String)> = def.map(|d| d.outputs.iter().map(|o| (o.name.clone(), o.ty.clone())).collect()).unwrap_or_default();
    for (port, _) in &outputs {
        out.push(Item::run("run.watch", format!("Watch {port}"), json!({"node": id, "port": port})).hover("Show this output in the Watched panel, and run"));
    }
    // a result node's Save Result…, once a run made it and the engine has somewhere to put it
    if let Some(run) = parts.run.node(&id.0)
        && !catalog.save_formats.is_empty()
        && matches!(run.state, Some(NodeState::Done | NodeState::Cached))
        && let Some(port) = def.filter(|d| d.flags.result).and_then(|d| d.outputs.first()).map(|o| o.name.clone())
    {
        out.push(Item::run("run.save", SAVE_RESULT, json!({"node": id, "port": port, "sheet": false, "ask": true})).hover(SAVE_HELP));
        let items = match &run.thumb {
            Some(plumcraft_proto::display::Rendition::Rgba8 { items, .. }) => *items,
            _ => 0,
        };
        if items > 1 {
            out.push(Item::run("run.save", CONTACT_SHEET, json!({"node": id, "port": port, "sheet": true, "ask": true})).hover(SHEET_HELP));
        }
    }
    // detach from a bundle
    if plumcraft_session::bundle_cmds::has_bundle(graph, catalog, id)
        && let Some(name) = def.and_then(plumcraft_session::bundle_cmds::bundle_name)
    {
        let hover = format!("Copy the {}'s values into this node's widgets and disconnect its {} input", name.to_lowercase(), name.to_lowercase());
        out.push(Item::run("bundle.detach", format!("Detach from {name}"), json!({"node": id})).hover(hover));
    }
    // expand into what it is made of: into a group is the usual choice, so it comes first
    if plumcraft_session::expand_cmds::has_expansion(catalog, &node.kind) {
        let name = plumcraft_session::display_name(catalog, &node.kind);
        let note = plumcraft_session::expand_cmds::expansion_of(catalog, &node.kind).map(|e| e.note).unwrap_or_default();
        let hover = |what: &str| if note.is_empty() { what.to_owned() } else { format!("{what}\n\n{note}") };
        out.push(Item::separator());
        out.push(
            Item::run("node.expand", format!("Expand into Group {name}"), json!({"node": id, "group": name}))
                .hover(hover("Replace this node with the nodes it is made of, grouped, with its settings promoted")),
        );
        out.push(Item::run("node.expand", "Expand without a Group", json!({"node": id})).hover(hover("Replace this node with the nodes it is made of")));
    }
    // a document output opens in its craft's app (a verified copy)
    for (port, ty) in &outputs {
        let Some(app) = parts.doc_apps.iter().find(|a| &a.wire == ty) else { continue };
        let item = Item::run("doc.open", format!("Open {port} in {}", app.label), json!({"node": id, "port": port}));
        out.push(match &app.program {
            Err(e) => item.disabled(Some(format!("{} isn't installed: {e}", app.label))),
            Ok(_) => item.hover(format!("Open a copy of this document in {} (runs first when it has no value yet)", app.label)),
        });
    }
    // an output the Watched panel can't show opens in another app
    for (port, ty) in &outputs {
        let media = catalog.wire_type(ty).map(|w| w.media).unwrap_or_default();
        if plumcraft_session::open_cmds::has_viewer(media) || parts.doc_apps.iter().any(|a| &a.wire == ty) {
            continue;
        }
        let target = json!({"node": id, "port": port});
        out.push(
            Item::run("value.open", format!("Open {port}"), target.clone())
                .hover("Open a copy in the default app for its type (runs first when it has no value yet)"),
        );
        let mut with = Vec::new();
        for app in parts.recent_apps {
            let mut p = target.clone();
            p["app"] = json!(app);
            with.push(Item::run("value.open", plumcraft_session::open_cmds::app_name(app), p).hover(app.display().to_string()));
        }
        if !parts.recent_apps.is_empty() {
            with.push(Item::separator());
        }
        let mut p = target.clone();
        p["choose"] = json!(true);
        with.push(Item::run("value.open", "Choose an App…", p));
        out.push(Item::submenu(&format!("value.open:{port}"), format!("Open {port} with"), with));
    }
    out.push(Item::run("node.duplicate", "Duplicate", json!({"nodes": targets})));
    out.push(Item::run("node.remove", "Remove", json!({"nodes": [id]})));
    out.extend(group_node_items(parts, facts, id, def));
    out
}

/// A node menu's group items: group the selection, then for each group the node is in, promote
/// an input, expose an output, the export mark, the group's parameters and the group itself.
fn group_node_items(parts: &Parts<'_>, facts: &Facts, id: &NodeId, def: Option<&NodeDef>) -> Vec<Item> {
    let graph = parts.graph;
    let catalog = parts.catalog;
    let (nodes, groups): (Vec<NodeId>, Vec<GroupId>) =
        if facts.selected.contains(id) { (facts.selected.clone(), facts.selected_groups.clone()) } else { (vec![id.clone()], Vec::new()) };
    let count = nodes.len() + groups.len();
    let what = if count == 1 { "this node".to_owned() } else { format!("{count} selected") };
    let mut out = vec![
        Item::separator(),
        Item::run("group.create", format!("Group {what}"), json!({"nodes": nodes, "groups": groups})).hover("Make a group of them (rename it from its header)"),
        Item::run("group.create", format!("Collapse {what} into a group"), json!({"nodes": nodes, "groups": groups, "collapsed": true}))
            .hover("Group them and show the group as one node"),
    ];
    let rows = def.map(|d| shown_rows(d, id, graph).0).unwrap_or_default();
    for gid in graph.enclosing_groups(id) {
        let Some(grp) = graph.group(&gid) else { continue };
        out.push(Item::separator());
        let labels: std::collections::BTreeSet<String> = grp.promoted.iter().map(|p| p.label.clone()).collect();
        let mut promote = Vec::new();
        for row in &rows {
            let at = PortRef::new(id.clone(), row.key.clone());
            match grp.promoted.iter().find(|p| p.port == at) {
                Some(p) => promote.push(Item::run(
                    "group.unpromote",
                    format!("[x] {} (as {}): unpromote", row.label, p.label),
                    json!({"group": gid, "label": p.label}),
                )),
                None => {
                    let label = unique_name(row.key.rsplit('.').next().unwrap_or(&row.key), &labels);
                    let doc = row.port.tooltip.clone().unwrap_or_default();
                    promote.push(
                        Item::run("group.promote", row.label.clone(), json!({"group": gid, "port": at, "label": label, "doc": doc}))
                            .hover(format!("Show it on {} as {label}", grp.name)),
                    );
                }
            }
        }
        out.push(Item::submenu(&format!("group.promote:{gid}"), format!("Promote input to {}", grp.name), promote));
        if let Some(def) = def {
            let names: std::collections::BTreeSet<String> = grp.outputs.iter().map(|o| o.name.clone()).collect();
            let mut outs = Vec::new();
            for o in &def.outputs {
                let from = PortRef::new(id.clone(), o.name.clone());
                match grp.outputs.iter().find(|x| x.from == from) {
                    Some(x) => outs.push(Item::run(
                        "group.removeOutput",
                        format!("[x] {} (as {}): remove", port_label(o), x.name),
                        json!({"group": gid, "name": x.name}),
                    )),
                    None => {
                        let name = unique_name(&o.name, &names);
                        outs.push(
                            Item::run("group.output", port_label(o), json!({"group": gid, "from": from, "name": name}))
                                .hover(format!("Show it as {}'s output {name}", grp.name)),
                        );
                    }
                }
            }
            out.push(Item::submenu(&format!("group.output:{gid}"), format!("Output of {}", grp.name), outs));
        }
        // the export mark, for node types that take one (or one a file marked)
        let on = grp.checkpoints.contains(id);
        if let Some(def) = def.filter(|d| d.export.markable || on) {
            let word = Some(def.export.mark.as_str()).filter(|m| !m.is_empty()).or(catalog.text(texts::EXPORT_MARK)).unwrap_or("export mark");
            let word: String = word.chars().next().map_or_else(String::new, |c| c.to_uppercase().chain(word.chars().skip(1)).collect());
            let label = if on { format!("[x] {word} in {}", grp.name) } else { format!("{word} in {}", grp.name) };
            out.push(
                Item::run("group.checkpoint", label, json!({"group": gid, "node": id, "on": !on}))
                    .hover("Marks this step for the group's export; a run ignores it"),
            );
        }
        if let Some(p) = parameters_menu(parts, &gid) {
            out.push(p);
        }
        out.push(Item::submenu(&format!("group:{gid}"), format!("Group {}", grp.name), group_items(&gid, &grp.name)));
    }
    out
}

/// "Parameters of <group> ›": an item per promoted parameter, opening its editor (a window: a
/// text field can't live in a menu). None without promoted parameters.
fn parameters_menu(parts: &Parts<'_>, gid: &GroupId) -> Option<Item> {
    let grp = parts.graph.group(gid)?;
    if grp.promoted.is_empty() {
        return None;
    }
    let items = grp
        .promoted
        .iter()
        .map(|pr| {
            Item::run("name.edit", format!("Rename {}…", pr.label), json!({"promoted": {"group": gid, "label": pr.label}}))
                .hover("Rename this promoted parameter")
        })
        .collect();
    Some(Item::submenu(&format!("group.parameters:{gid}"), format!("Parameters of {}", grp.name), items))
}

/// An open group's header menu (needs only the graph).
pub fn group_menu(graph: &plumcraft_graph::Graph, gid: &GroupId) -> Vec<Item> {
    graph.group(gid).map(|g| group_items(gid, &g.name)).unwrap_or_default()
}

/// A group's own items: rename and ungroup (collapse, expand and enter are its header's buttons).
fn group_items(gid: &GroupId, name: &str) -> Vec<Item> {
    vec![
        Item::run("name.edit", "Rename…", json!({"group": gid})).hover(format!("Rename {name}")),
        Item::run("group.ungroup", "Ungroup", json!({"group": gid})).hover("Dissolve the group; its nodes stay"),
    ]
}

/// A collapsed group's menu.
fn proxy_menu(parts: &Parts<'_>, gid: &GroupId) -> Vec<Item> {
    let graph = parts.graph;
    let Some(grp) = graph.group(gid) else { return Vec::new() };
    let mut out = vec![
        Item::line(Look::Note, format!("group {} ({}, {} nodes)", grp.name, gid, graph.group_members(gid).len())),
        Item::run("group.collapse", "Expand", json!({"group": gid, "collapsed": false})).hover("Show the group's nodes in place"),
    ];
    out.extend(group_items(gid, &grp.name));
    if let Some(p) = parameters_menu(parts, gid) {
        out.push(p);
    }
    if !grp.outputs.is_empty() {
        let items = grp
            .outputs
            .iter()
            .map(|o| Item::run("group.removeOutput", format!("Remove {}", o.name), json!({"group": gid, "name": o.name})).hover(format!("{}", o.from)))
            .collect();
        out.push(Item::submenu(&format!("group.outputs:{gid}"), "Outputs", items));
    }
    out.push(Item::separator());
    for o in proxy_outputs(graph, parts.catalog, gid) {
        out.push(
            Item::run("run.watch", format!("Watch {}", o.label), json!({"node": o.port.node, "port": o.port.port}))
                .hover("Show this output in the Watched panel, and run"),
        );
    }
    out
}

/// A wire's menu.
fn wire_menu(parts: &Parts<'_>, to: &PortRef, bend: Option<&Value>) -> Vec<Item> {
    let graph = parts.graph;
    let Some(w) = graph.wire_into(to) else { return Vec::new() };
    let from = &w.from;
    let watch = json!({"node": from.node, "port": from.port});
    let mut out = vec![
        Item::line(Look::Note, format!("{}.{} -> {}.{}", from.node, from.port, to.node, to.port)),
        Item::run("run.watch", "Watch", watch.clone()).hover("Show this wire's value in the Watched panel"),
        Item::run("view.popout", "Pop Out", watch).hover("Show this wire's value in a window of its own, updated by every run"),
    ];
    let label = if w.slice.is_some() { "Edit Slice…" } else { "Slice…" };
    out.push(
        Item::run("name.edit", label, json!({"slice": to})).hover("Take part of the list this wire carries: start:stop:step (1:-1:2, :-1) or an index (-1)"),
    );
    if w.slice.is_some() {
        out.push(Item::run("wire.slice", "Clear Slice", json!({"to": to, "slice": null})));
    }
    let at = bend.and_then(|b| b.get("at")).and_then(Value::as_array).and_then(|a| Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?]));
    let bend_item = match (at, bend) {
        (Some(p), _) => {
            // the point where the press was, among the wire's bend points in order along it
            let start = parts.graph.node(&w.from.node).map_or([0.0, 0.0], |n| [f64::from(n.pos[0]), f64::from(n.pos[1])]);
            let end = parts.graph.node(&to.node).map_or([1.0, 0.0], |n| [f64::from(n.pos[0]), f64::from(n.pos[1])]);
            let dir = [end[0] - start[0], end[1] - start[1]];
            let along = |q: [f64; 2]| (q[0] - start[0]) * dir[0] + (q[1] - start[1]) * dir[1];
            let point = [p[0].round() as i32, p[1].round() as i32];
            let mut via = w.via.clone();
            let k = via.iter().position(|v| along([f64::from(v[0]), f64::from(v[1])]) > along(p)).unwrap_or(via.len());
            via.insert(k, point);
            Item::run("wire.route", "Add Bend Point", json!({"to": to, "via": via}))
        }
        (None, Some(via)) => Item::run("wire.route", "Add Bend Point", json!({"to": to, "via": via})),
        (None, None) => {
            // no point given: at the wire's middle, between its ends
            let mut via = w.via.clone();
            let ends = (parts.graph.node(&w.from.node).map(|n| n.pos), parts.graph.node(&to.node).map(|n| n.pos));
            if let (Some(a), Some(b)) = ends {
                via.push([((a[0] + b[0]) / 2.0) as i32 + 110, ((a[1] + b[1]) / 2.0) as i32 + 40]);
            }
            Item::run("wire.route", "Add Bend Point", json!({"to": to, "via": via}))
        }
    };
    out.push(bend_item.hover("Bend the wire here: drag the point to shape it"));
    if !w.via.is_empty() {
        out.push(Item::run("wire.route", "Straighten", json!({"to": to, "via": []})).hover("Remove the bend points"));
    }
    out.push(Item::run("name.edit", "Label All Uses…", json!({"label": from})).hover("Draw every wire from this output as a net label, as a schematic does"));
    out.push(Item::run("wire.disconnect", "Disconnect", json!({"to": to})));
    out
}

/// An input's menu: name it, or rename or unname its name.
fn input_menu(parts: &Parts<'_>, at: &PortRef) -> Vec<Item> {
    let graph = parts.graph;
    if let Some(name) = graph.name_of_port(at) {
        return vec![
            Item::run("name.edit", "Rename Port…", json!({"port": at})),
            Item::run("graph.port.unname", "Remove Name", json!({"name": name})).hover("The port stops being one of the graph's parameters; its value stays"),
        ];
    }
    let has_widget = graph
        .node(&at.node)
        .and_then(|n| parts.catalog.node(&n.kind))
        .and_then(|d| crate::rows::input_rows(d, &at.node, graph).into_iter().find(|r| r.key == at.port))
        .is_some_and(|r| r.port.has_widget());
    if has_widget && graph.wire_into(at).is_none() {
        vec![
            Item::run("name.edit", "Name This Port…", json!({"port": at}))
                .hover("Make it one of the graph's parameters, set from outside by name (run --set NAME=VALUE)"),
        ]
    } else {
        vec![Item::line(Look::Text, "A wired input can't be named")]
    }
}

/// A frame's menu: its title, a colour, remove.
pub fn frame_menu(graph: &plumcraft_graph::Graph, id: &str) -> Vec<Item> {
    let fid = plumcraft_graph::FrameId(id.to_owned());
    let Some(frame) = graph.frames.get(&fid) else { return Vec::new() };
    let mut colours: Vec<Item> = SWATCHES
        .iter()
        .map(|(name, hex)| {
            let mut i =
                Item::run("frame.set", format!("■ {name}"), json!({"frame": fid, "color": hex})).look(Look::Selected, frame.color.as_deref() == Some(*hex));
            i.color = Some((*hex).to_owned());
            i
        })
        .collect();
    colours.push(Item::run("frame.set", "Theme accent", json!({"frame": fid, "color": null})).look(Look::Selected, frame.color.is_none()));
    vec![
        Item::line(Look::Note, format!("frame {fid}")),
        Item::run("name.edit", "Rename…", json!({"frame": fid})),
        Item::submenu(&format!("frame.colour:{fid}"), "Colour", colours),
        Item::run("frame.remove", "Remove Frame", json!({"frame": fid})).hover("Its nodes stay"),
    ]
}

/// A net tag's menu.
pub fn net_menu(graph: &plumcraft_graph::Graph, key: &plumcraft_graph::NetKey, to: Option<&PortRef>) -> Vec<Item> {
    let net = graph.net(key);
    let from = net.map(|n| n.from.to_string()).unwrap_or_default();
    let describe = match key.group.as_ref().and_then(|g| graph.group(g)) {
        Some(g) => format!("{} (in group {})", key.name, g.name),
        None => key.name.clone(),
    };
    let go = match (to, net) {
        (Some(_), Some(n)) => Item::run("canvas.reveal", "Go to Driver", json!({"node": n.from.node})),
        _ => Item::run("canvas.reveal", "Go to Next Use", json!({"net": key.name, "group": key.group})),
    };
    let mut out = vec![Item::line(Look::Note, format!("Net {describe}, from {from}")), go];
    if let Some(n) = net {
        let watch = json!({"node": n.from.node, "port": n.from.port});
        out.push(Item::run("run.watch", "Watch", watch.clone()).hover("Show this net's value in the Watched panel"));
        out.push(Item::run("view.popout", "Pop Out", watch).hover("Show this net's value in a window of its own, updated by every run"));
    }
    out.push(Item::run("net.select", "Select Net", json!({"name": key.name, "group": key.group})).hover("Highlight every label of this net"));
    out.push(Item::run("name.edit", "Rename Net…", json!({"net": key.name, "group": key.group})));
    if let Some(to) = to {
        out.push(Item::run("wire.unlabel", "Draw as Wire", json!({"to": to})).hover("Draw this connection as a wire again"));
    }
    if to.is_none() || graph.net_wires(key).count() > 1 {
        let tos: Vec<&PortRef> = graph.net_wires(key).map(|w| &w.to).collect();
        out.push(Item::run("wire.unlabel", "Draw All as Wires", json!({"to": tos})).hover("Draw every connection of this net as a wire again"));
    }
    out
}

/// The model's generation: up by one whenever what it holds changed.
#[derive(Debug, Clone, Default)]
pub struct Revision {
    generation: u64,
    hash: Option<u64>,
}

impl Revision {
    /// The generation of `model`, after noting it: the same as last time unless it changed.
    pub fn note(&mut self, model: &Value) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        model.to_string().hash(&mut h);
        let hash = h.finish();
        if self.hash != Some(hash) {
            self.hash = Some(hash);
            self.generation += 1;
        }
        self.generation
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// `menu.get {context?}`: the menu bar (`{gen, menus}`), or a context menu (`{context, items}`).
pub fn get(session: &Session, facts: &Facts, params: &Value, revision: Option<&mut Revision>) -> Result<Value, String> {
    crate::control::check_fields(params, &["context"])?;
    match params.get("context") {
        None | Some(Value::Null) => {
            let menus = serde_json::to_value(bar(session, facts)).map_err(|e| e.to_string())?;
            let mut out = json!({"menus": menus});
            if let Some(r) = revision {
                out["gen"] = json!(r.note(&menus));
            }
            Ok(out)
        }
        Some(c) => {
            let cx = Context::parse(c)?;
            let list = if cx == Context::Toolbar { toolbar(session, facts) } else { context(&Parts::of(session), facts, &cx) };
            let items = serde_json::to_value(list).map_err(|e| e.to_string())?;
            Ok(json!({"context": cx.kind(), "items": items}))
        }
    }
}

#[cfg(test)]
mod tests {
    use plumcraft_session::EngineChoice;

    use super::*;

    fn find<'a>(items: &'a [Item], id: &str) -> Option<&'a Item> {
        items.iter().flat_map(Item::walk).find(|i| i.id == id)
    }

    #[test]
    fn node_add_offers_no_testing_kind() {
        // the maintainer, 2026-11-21, card 31 B: "hide Testing kinds from the palette"
        let c = plumcraft_text::engine().schema();
        let tree = add_tree(&c, None);
        let kinds: Vec<&str> = tree.iter().flat_map(Item::walk).filter_map(|i| i.params.as_ref()?.get("kind")?.as_str()).collect();
        assert!(kinds.contains(&"text.upper"), "{kinds:?}");
        assert!(!kinds.contains(&"text.widgets") && !kinds.contains(&"text.slow"), "{kinds:?}");
        assert!(find(&tree, "node.add:/Text/Testing").is_none(), "an empty Testing submenu");
    }

    #[test]
    fn shortcuts_parse_to_data() {
        assert_eq!(parse_shortcut("Cmd+Shift+Z"), Some(Shortcut { key: "Z".into(), mods: vec![Mod::Primary, Mod::Shift] }));
        assert_eq!(parse_shortcut("Shift+Cmd+Z"), parse_shortcut("Cmd+Shift+Z"));
        assert_eq!(parse_shortcut("Cmd+Comma").map(|s| s.key), Some("Comma".into()));
        assert_eq!(parse_shortcut("Opt+F12").map(|s| s.mods), Some(vec![Mod::Alt]));
        for bad in ["", "Cmd+", "Cmd+Nope", "Z+Y", "Hyper+Z", "Cmd+F0", "Cmd+F99", "Cmd+z"] {
            assert_eq!(parse_shortcut(bad), None, "{bad}");
        }
        for spec in command_specs() {
            if let Some(s) = spec.shortcut {
                assert!(parse_shortcut(s).is_some(), "{}: {s}", spec.id);
            }
        }
        // the most specific first: Redo before Undo, Save As before Save
        let table = shortcut_table();
        let at = |id: &str| table.iter().position(|b| b.command == id).unwrap();
        assert!(at("edit.redo") < at("edit.undo") && at("graph.saveAs") < at("graph.save"));
        assert!(table.iter().find(|b| b.command == "edit.undo").unwrap().text_field_keeps);
        assert!(!table.iter().find(|b| b.command == "graph.save").unwrap().text_field_keeps);
    }

    #[test]
    fn the_bar_has_the_registry_menus_in_order_with_roles() {
        let s = Session::default();
        let bar = bar(&s, &Facts::default());
        let titles: Vec<&str> = bar.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(&titles[..5], MENUS);
        assert!(titles.contains(&"Engine") && titles.contains(&"Help"));
        let quit = find(&bar, "app.quit").unwrap();
        assert_eq!((quit.role, quit.shortcut.clone()), (Some(Role::Quit), parse_shortcut("Cmd+Q")));
        assert_eq!(find(&bar, "app.settings").unwrap().role, Some(Role::Preferences));
        assert_eq!(find(&bar, "app.about").unwrap().role, Some(Role::About));
        assert_eq!(bar[1].role, Some(Role::EditMenu));
        // a fresh document has nothing to undo, and says why
        let undo = find(&bar, "edit.undo").unwrap();
        assert!(!undo.enabled && undo.reason.is_some());
        // the toggles carry their state
        assert_eq!(find(&bar, "view.runColors").unwrap().checked, Some(true));
    }

    #[test]
    fn undo_follows_the_document() {
        let mut s = Session::new(plumcraft_text::engine().schema());
        let undo = |s: &Session| find(&bar(s, &Facts::default()), "edit.undo").unwrap().enabled;
        assert!(!undo(&s));
        s.execute("node.add", &json!({"kind": "text.literal", "pos": [0, 0]})).unwrap();
        assert!(undo(&s));
    }

    #[test]
    fn the_revision_moves_only_when_the_model_changes() {
        let mut s = Session::new(plumcraft_text::engine().schema());
        let mut r = Revision::default();
        let model = |s: &Session| serde_json::to_value(bar(s, &Facts::default())).unwrap();
        let g1 = r.note(&model(&s));
        assert_eq!(r.note(&model(&s)), g1, "nothing changed");
        s.execute("node.add", &json!({"kind": "text.literal", "pos": [0, 0]})).unwrap();
        let g2 = r.note(&model(&s));
        assert!(g2 > g1);
        assert_eq!(r.note(&model(&s)), g2);
    }

    fn choice(engine: &str, label: &str, mode: &str, mode_label: &str) -> EngineChoice {
        EngineChoice { engine: engine.into(), mode: mode.into(), engine_label: label.into(), mode_label: mode_label.into() }
    }

    #[test]
    fn the_engine_menu_lists_what_this_device_cant_start_disabled_with_why() {
        let mut s = Session::default();
        // a desktop: every hosting there is can start, and the menu lists the app's choices only
        s.engines.choices = crate::engines::choices_for(&Capabilities::DESKTOP);
        let desk = Facts { caps: Capabilities::DESKTOP, ..Facts::default() };
        let engine = bar_menu(&s, &desk, "Engine");
        let radios: Vec<&Item> = engine.walk().into_iter().filter(|i| i.look == Look::Radio).collect();
        assert_eq!(radios.len(), s.engines.choices.len());
        assert!(radios.iter().all(|i| i.enabled));
        // a device that can't start programs: what it can start, and every process hosting
        // listed, disabled, saying why
        let caps = Capabilities { subprocess: false, ..Capabilities::DESKTOP };
        s.engines.choices = crate::engines::choices_for(&caps);
        let phone = Facts { caps, ..Facts::default() };
        let engine = bar_menu(&s, &phone, "Engine");
        let radios: Vec<&Item> = engine.walk().into_iter().filter(|i| i.look == Look::Radio).collect();
        assert_eq!(radios.len(), offers(&caps).len(), "every hosting is listed");
        let process = radios.iter().find(|i| i.params == Some(json!({"engine": "text", "mode": "process"}))).unwrap();
        assert!(!process.enabled);
        assert_eq!(process.reason.as_deref(), Some("this device can't start programs"));
        let inproc = radios.iter().find(|i| i.params == Some(json!({"engine": "text", "mode": "inproc"}))).unwrap();
        assert!(inproc.enabled && inproc.reason.is_none());
        // picking automatically: an engine nothing here can start is one disabled item
        s.engines.auto = true;
        let engine = bar_menu(&s, &phone, "Engine");
        let audio = engine.walk().into_iter().find(|i| i.params == Some(json!({"engine": "audio"}))).unwrap();
        assert!(!audio.enabled && audio.reason.is_some());
        let text = engine.walk().into_iter().find(|i| i.params == Some(json!({"engine": "text"}))).unwrap();
        assert!(text.enabled);
        // and the app's own list, with a hosting the app doesn't offer, gets nothing it didn't name
        s.engines.auto = false;
        s.engines.choices = vec![choice("text", "Text", "inproc", "Rust · in-process")];
        let engine = bar_menu(&s, &desk, "Engine");
        assert_eq!(engine.walk().into_iter().filter(|i| i.look == Look::Radio).count(), 1);
    }

    #[test]
    fn context_menus_name_their_commands() {
        let mut s = Session::new(plumcraft_text::engine().schema());
        let a = s.execute("node.add", &json!({"kind": "text.literal", "pos": [0, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
        let u = s.execute("node.add", &json!({"kind": "text.upper", "pos": [300, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
        s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": u, "port": "text"}})).unwrap();
        let f = Facts::default();
        let node = context(&Parts::of(&s), &f, &Context::Node { node: NodeId::from(a.as_str()) });
        assert!(find(&node, "node.duplicate").is_some_and(|i| i.params == Some(json!({"nodes": [a]}))));
        // a name is edited in place through the core (N4 slice 3), no longer the shell's own item
        assert!(find(&node, "name.edit").is_some_and(|i| i.params == Some(json!({"node": a}))));
        assert!(node.iter().flat_map(Item::walk).all(|i| !i.is_action() || i.command.is_some()), "every item the person can choose is a command");
        let wire = context(&Parts::of(&s), &f, &Context::Wire { to: PortRef::new(u.as_str(), "text"), bend: None });
        assert!(find(&wire, "wire.disconnect").is_some());
        assert!(context(&Parts::of(&s), &f, &Context::Node { node: NodeId::from("gone") }).is_empty());
        // hostile contexts are refused, not fatal
        for bad in [
            json!(null),
            json!(3),
            json!({}),
            json!({"kind": "node"}),
            json!({"kind": "nope"}),
            json!({"kind": "wire", "to": "x"}),
            json!({"kind": "node", "node": "x".repeat(300)}),
        ] {
            assert!(Context::parse(&bad).is_err(), "{bad}");
        }
        let got = get(&s, &f, &json!({"context": {"kind": "node", "node": a}}), None).unwrap();
        assert_eq!(got["context"], "node");
        assert!(get(&s, &f, &json!({"nope": 1}), None).is_err());
    }
}
