//! The node icons: the Lucide SVGs a node type names (`NodeDef::icon`; ISC,
//! `crates/ui/assets/icons/LICENSE-lucide.txt`), embedded once for every shell (N4 slice 2). The
//! egui shell draws them through `egui_extras` (`plumcraft-ui`'s `icons`); a native shell asks for
//! them with `icon.get` and draws their paths with its own 2D stack. An engine may name any Lucide
//! icon: one this set doesn't have is drawn as [`FALLBACK`].

/// The icon shown for a name the set doesn't have.
pub const FALLBACK: &str = "box";

pub static ICONS: &[(&str, &[u8])] = &[
    ("activity", include_bytes!("../assets/icons/activity.svg")),
    ("blocks", include_bytes!("../assets/icons/blocks.svg")),
    ("bookmark", include_bytes!("../assets/icons/bookmark.svg")),
    ("box", include_bytes!("../assets/icons/box.svg")),
    ("boxes", include_bytes!("../assets/icons/boxes.svg")),
    ("calculator", include_bytes!("../assets/icons/calculator.svg")),
    ("chevron-down", include_bytes!("../assets/icons/chevron-down.svg")),
    ("chevron-right", include_bytes!("../assets/icons/chevron-right.svg")),
    ("crop", include_bytes!("../assets/icons/crop.svg")),
    ("dice-5", include_bytes!("../assets/icons/dice-5.svg")),
    ("ellipsis-vertical", include_bytes!("../assets/icons/ellipsis-vertical.svg")),
    ("eye", include_bytes!("../assets/icons/eye.svg")),
    ("flag", include_bytes!("../assets/icons/flag.svg")),
    ("frame", include_bytes!("../assets/icons/frame.svg")),
    ("grid-3x3", include_bytes!("../assets/icons/grid-3x3.svg")),
    ("hash", include_bytes!("../assets/icons/hash.svg")),
    ("image", include_bytes!("../assets/icons/image.svg")),
    ("layers", include_bytes!("../assets/icons/layers.svg")),
    ("menu", include_bytes!("../assets/icons/menu.svg")),
    ("paintbrush", include_bytes!("../assets/icons/paintbrush.svg")),
    ("palette", include_bytes!("../assets/icons/palette.svg")),
    ("pen-tool", include_bytes!("../assets/icons/pen-tool.svg")),
    ("pencil", include_bytes!("../assets/icons/pencil.svg")),
    ("pipette", include_bytes!("../assets/icons/pipette.svg")),
    ("ruler", include_bytes!("../assets/icons/ruler.svg")),
    ("save", include_bytes!("../assets/icons/save.svg")),
    ("scan-line", include_bytes!("../assets/icons/scan-line.svg")),
    ("scaling", include_bytes!("../assets/icons/scaling.svg")),
    ("shapes", include_bytes!("../assets/icons/shapes.svg")),
    ("sigma", include_bytes!("../assets/icons/sigma.svg")),
    ("sliders-horizontal", include_bytes!("../assets/icons/sliders-horizontal.svg")),
    ("sparkles", include_bytes!("../assets/icons/sparkles.svg")),
    ("spline", include_bytes!("../assets/icons/spline.svg")),
    ("split", include_bytes!("../assets/icons/split.svg")),
    ("wand-sparkles", include_bytes!("../assets/icons/wand-sparkles.svg")),
    ("waves", include_bytes!("../assets/icons/waves.svg")),
    ("workflow", include_bytes!("../assets/icons/workflow.svg")),
    ("wrench", include_bytes!("../assets/icons/wrench.svg")),
];

/// Icon `name`'s SVG text, else the fallback's.
pub fn svg(name: &str) -> (&'static str, &'static str) {
    let find = |n: &str| ICONS.iter().find(|(k, _)| *k == n).and_then(|(k, b)| std::str::from_utf8(b).ok().map(|s| (*k, s)));
    find(name).or_else(|| find(FALLBACK)).unwrap_or((FALLBACK, ""))
}

/// `icon.get {names}`' reply: `{icons: {name: {svg, as}}}`, `as` the name drawn (the fallback's
/// for a name the set doesn't have). At most 256 names.
pub fn reply(params: &serde_json::Value) -> Result<serde_json::Value, String> {
    crate::control::check_fields(params, &["names"])?;
    let names = params.get("names").and_then(serde_json::Value::as_array).filter(|a| a.len() <= 256).ok_or("names must be a list of at most 256 icon names")?;
    let mut out = serde_json::Map::new();
    for n in names {
        let n = n.as_str().filter(|s| s.len() <= 128).ok_or("names must be icon names (strings)")?;
        let (drawn, svg) = svg(n);
        out.insert(n.to_owned(), serde_json::json!({"svg": svg, "as": drawn}));
    }
    Ok(serde_json::json!({"icons": out}))
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_icon_is_svg_text_and_an_unknown_name_is_the_fallback() {
        for (name, bytes) in super::ICONS {
            let s = std::str::from_utf8(bytes).unwrap();
            assert!(s.contains("<svg"), "{name}");
        }
        assert_eq!(super::svg("no-such-icon").0, super::FALLBACK);
        let r = super::reply(&serde_json::json!({"names": ["activity", "nope"]})).unwrap();
        assert_eq!(r["icons"]["nope"]["as"], "box");
        assert!(super::reply(&serde_json::json!({"names": [5]})).is_err());
        assert!(super::reply(&serde_json::json!({})).is_err());
    }
}
