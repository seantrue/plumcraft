//! Step 3 (docs/research/15-multi-engine.md, sections 3.2 to 3.4): one catalogue from several,
//! which engine runs a node, and which wires may cross from one engine to another. Pure: the
//! sub-engines' catalogues in, answers out.

use std::collections::BTreeMap;

use md5::{Digest, Md5};
use plumcraft_proto::PROTOCOL;
use plumcraft_proto::catalogue::{Compat, SchemaReply, core_types, scalars};
use plumcraft_proto::session::EngineInfo;

/// The coordinator's engine id, in a merged catalogue.
pub const ENGINE_ID: &str = "plumcraft";

/// A catalogue merged from several.
#[derive(Debug, Clone, Default)]
pub struct Merged {
    /// What the coordinator's `schema` answers. Every node lists the engines that offer it
    /// (`NodeDef.engines`), the default first.
    pub schema: SchemaReply,
    /// Node kind → every engine that offers it, in the order the engines were added.
    pub owners: BTreeMap<String, Vec<String>>,
    /// Conflicts, for the people to fix: a wire type two engines declare differently.
    pub warnings: Vec<String>,
}

/// Merge `parts` (engine id, its catalogue), in order: what two engines both declare is taken
/// from the first, and a wire type they declare with different media is a warning.
pub fn catalogue(parts: &[(String, SchemaReply)]) -> Merged {
    let mut m = Merged::default();
    let s = &mut m.schema;
    s.v = PROTOCOL;
    let mut compat: Vec<Compat> = Vec::new();
    let mut digest = Md5::new();
    let mut salts = Vec::new();
    for (id, part) in parts {
        digest.update(format!("{id}\0{}\0", part.engine.schema_digest).as_bytes());
        salts.push(format!("{id}={}", part.engine.salt));
        for (k, v) in &part.engine.components {
            s.engine.components.entry(format!("{id}.{k}")).or_insert_with(|| v.clone());
        }
        s.engine.components.entry(id.clone()).or_insert_with(|| part.engine.version.clone());
        for w in &part.wire_types {
            match s.wire_types.iter().find(|x| x.id == w.id) {
                None => s.wire_types.push(w.clone()),
                Some(x) if x.media != w.media => m.warnings.push(format!(
                    "wire type {} is {:?} in {} but {:?} in {id}; {id}'s values of it can't cross",
                    w.id,
                    x.media,
                    owner_of_type(parts, &w.id),
                    w.media
                )),
                Some(_) => {}
            }
        }
        for c in part.compat_rules() {
            if !compat.contains(&c) {
                compat.push(c);
            }
        }
        for f in &part.families {
            if !s.families.iter().any(|x| x.id == f.id) {
                s.families.push(f.clone());
            }
        }
        for n in &part.nodes {
            m.owners.entry(n.id.clone()).or_default().push(id.clone());
            match s.nodes.iter_mut().find(|x| x.id == n.id) {
                Some(x) => x.engines.push(id.clone()),
                None => {
                    let mut n = n.clone();
                    n.engines = vec![id.clone()];
                    s.nodes.push(n);
                }
            }
        }
        for (k, v) in &part.expansions {
            s.expansions.entry(k.clone()).or_insert_with(|| v.clone());
        }
        for (k, v) in &part.templates {
            s.templates.entry(k.clone()).or_insert_with(|| v.clone());
        }
        for f in &part.save_formats {
            if !s.save_formats.iter().any(|x| x.id == f.id) {
                s.save_formats.push(f.clone());
            }
        }
        for (k, v) in &part.texts {
            s.texts.entry(k.clone()).or_insert_with(|| v.clone());
        }
        for (k, v) in &part.x {
            s.x.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    s.compat = Some(compat);
    let names: Vec<&str> = parts.iter().map(|(_, p)| if p.engine.name.is_empty() { p.engine.id.as_str() } else { p.engine.name.as_str() }).collect();
    s.engine = EngineInfo {
        id: ENGINE_ID.into(),
        name: names.join(" + "),
        version: env!("CARGO_PKG_VERSION").into(),
        components: std::mem::take(&mut s.engine.components),
        salt: format!("coordinator:{}+{}", env!("CARGO_PKG_VERSION"), salts.join("+")),
        schema_digest: format!("{:x}", digest.finalize()),
    };
    m
}

fn owner_of_type<'a>(parts: &'a [(String, SchemaReply)], ty: &str) -> &'a str {
    parts.iter().find(|(_, p)| p.wire_type(ty).is_some()).map_or("", |(id, _)| id.as_str())
}

/// Which engine runs a node of `kind` (section 3.3): the node's own `engine` when it names one
/// that offers the kind; else the only engine that offers it; else, of several, the first in
/// the workflow's `engines` list, then the first added. `Err` says why none can.
pub fn owner(kind: &str, pinned: Option<&str>, preferred: &[String], owners: &BTreeMap<String, Vec<String>>) -> Result<String, String> {
    let offered: &[String] = owners.get(kind).map_or(&[], Vec::as_slice);
    if let Some(p) = pinned.filter(|p| !p.is_empty()) {
        return if offered.iter().any(|o| o == p) {
            Ok(p.to_owned())
        } else if owners.values().flatten().any(|o| o == p) {
            Err(format!("engine {p} has no node {kind}"))
        } else {
            Err(format!("engine {p} is not connected"))
        };
    }
    match offered {
        [] => Err(format!("no connected engine has a node {kind}")),
        [one] => Ok(one.clone()),
        several => {
            preferred.iter().find(|p| several.contains(p)).or_else(|| several.first()).cloned().ok_or_else(|| format!("no connected engine has a node {kind}"))
        }
    }
}

/// Whether a value of type `ty` may go from engine `from` to engine `to` (section 3.4): a
/// scalar or a core type always; another type only when both engines declare it with the same
/// media. The same engine: always (its own compat rules decide).
pub fn portable(ty: &str, from: (&str, &SchemaReply), to: (&str, &SchemaReply)) -> Result<(), String> {
    if from.0 == to.0 || scalars::is_scalar(ty) || core_types::is_core(ty) {
        return Ok(());
    }
    match (from.1.wire_type(ty), to.1.wire_type(ty)) {
        (Some(a), Some(b)) if a.media == b.media => Ok(()),
        (Some(a), Some(b)) => Err(format!("{ty} is {:?} in {} but {:?} in {}", a.media, from.0, b.media, to.0)),
        (_, None) => Err(format!("{} has no type {ty}, so a {ty} can't go from {} to it", to.0, from.0)),
        (None, _) => Err(format!("{} has no type {ty}", from.0)),
    }
}
