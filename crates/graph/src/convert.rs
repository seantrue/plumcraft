//! The core Convert node in a graph (`docs/research/20-convert.md`).
//!
//! A Convert node's two ports have no type of their own: each takes its type from what it is
//! wired to. Its input accepts a value some path of converters can take to every input its
//! output feeds (to any converter at all while the output is unwired), and its output feeds an
//! input that a path can reach from its input's type (any converter's result while the input is
//! unwired). A Convert never feeds another Convert.
//!
//! Once both ends are wired the node keeps the best path, as its kinds, in the workflow
//! ([`PIN`]): connecting another engine later doesn't reroute it. The pin changes only when it
//! no longer fits the types it is wired between (a new wire, say), or when asked
//! ([`Graph::repin_convert`]). A pin whose kinds have gone, with the wires unchanged, is a wire
//! problem; the node then runs the best path there is until it is repinned.

use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::convert::{self, KIND, PIN, PORT, Path};
use serde_json::Value;

use crate::ports::resolve_input;
use crate::{Graph, GraphError, NodeId, PortRef, Result};

impl Graph {
    /// Is node `id` a Convert node?
    pub fn is_convert(&self, id: &NodeId) -> bool {
        self.nodes.get(id).is_some_and(|n| n.kind == KIND)
    }

    /// The type wired into Convert `id`'s input, when it is wired from something typed.
    pub fn convert_from_type(&self, catalog: &SchemaReply, id: &NodeId) -> Option<String> {
        let w = self.wire_into(&PortRef::new(id.clone(), PORT))?;
        let n = self.nodes.get(&w.from.node)?;
        let def = catalog.node(&n.kind)?;
        def.output(&w.from.port).filter(|_| n.kind != KIND).map(|p| p.ty.clone())
    }

    /// The types of the inputs Convert `id`'s output feeds, but the one into `except`.
    pub fn convert_to_types(&self, catalog: &SchemaReply, id: &NodeId, except: Option<&PortRef>) -> Vec<String> {
        self.wires
            .iter()
            .filter(|w| w.from.node == *id && Some(&w.to) != except)
            .filter_map(|w| {
                let n = self.nodes.get(&w.to.node)?;
                let def = catalog.node(&n.kind)?;
                resolve_input(def, n, &w.to.node, &w.to.port).ok().map(|r| r.port.ty.clone())
            })
            .collect()
    }

    /// The kinds Convert `id` keeps as its path, if any.
    pub fn convert_pin(&self, id: &NodeId) -> Option<Vec<String>> {
        let v = self.nodes.get(id)?.extra.get(PIN)?.as_array()?;
        Some(v.iter().filter_map(|k| k.as_str().map(str::to_owned)).collect())
    }

    /// The path Convert `id` runs: its pin when that fits the types it is wired between, else
    /// the best path there is. `None` until both ends are wired, or when no path fits.
    pub fn convert_path<'c>(&self, catalog: &'c SchemaReply, id: &NodeId) -> Option<Path<'c>> {
        let from = self.convert_from_type(catalog, id)?;
        let to = self.convert_to_types(catalog, id, None);
        if to.is_empty() {
            return None;
        }
        let fits = |t: &str| to.iter().all(|u| catalog.accepts(t, u));
        if let Some(pin) = self.convert_pin(id)
            && let Some(p) = catalog.convert_path_of(&from, &pin)
            && fits(p.last().map_or(from.as_str(), |c| c.to))
        {
            return Some(p);
        }
        catalog.convert_paths_where(&from, fits).into_iter().next()
    }

    /// Does Convert `id`'s pin fit the types it is wired between? `None` when it has no pin or
    /// isn't wired at both ends.
    pub fn convert_pin_fits(&self, catalog: &SchemaReply, id: &NodeId) -> Option<bool> {
        let pin = self.convert_pin(id)?;
        let from = self.convert_from_type(catalog, id)?;
        let to = self.convert_to_types(catalog, id, None);
        if to.is_empty() {
            return None;
        }
        let path = catalog.convert_path_of(&from, &pin);
        Some(path.is_some_and(|p| {
            let last = p.last().map_or(from.as_str(), |c| c.to);
            to.iter().all(|u| catalog.accepts(last, u))
        }))
    }

    /// Give Convert `id` the best path for the types it is wired between, unless its pin
    /// already fits (or `force`). Returns whether the pin changed. Nothing for another node, or
    /// a Convert not wired at both ends.
    pub fn repin_convert(&mut self, catalog: &SchemaReply, id: &NodeId, force: bool) -> bool {
        if !self.is_convert(id) || (!force && self.convert_pin_fits(catalog, id) == Some(true)) {
            return false;
        }
        let Some(from) = self.convert_from_type(catalog, id) else { return false };
        let to = self.convert_to_types(catalog, id, None);
        if to.is_empty() {
            return false;
        }
        let Some(best) = catalog.convert_paths_where(&from, |t| to.iter().all(|u| catalog.accepts(t, u))).into_iter().next() else { return false };
        let kinds: Vec<Value> = best.iter().map(|c| Value::from(c.kind)).collect();
        let Some(node) = self.nodes.get_mut(id) else { return false };
        let new = Value::Array(kinds);
        if node.extra.get(PIN) == Some(&new) {
            return false;
        }
        node.extra.insert(PIN.to_owned(), new);
        self.touch();
        true
    }

    /// The type rules for a wire with a Convert end (module docs). `out_ty` is the source
    /// port's type, `in_ty` the input's.
    pub(crate) fn check_convert_wire(&self, catalog: &SchemaReply, from: &PortRef, out_ty: &str, to: &PortRef, in_ty: &str) -> Result<()> {
        let refuse = |reason: String| Err(GraphError::Convert { from: from.to_string(), to: to.to_string(), reason });
        let (feeds, fed) = (self.is_convert(&from.node), self.is_convert(&to.node));
        if feeds && fed {
            return refuse("a Convert can't feed another Convert".into());
        }
        if fed {
            let targets = self.convert_to_types(catalog, &to.node, None);
            if targets.is_empty() {
                if !catalog.converts_from(out_ty) {
                    return refuse(format!("no connected engine converts {out_ty}"));
                }
            } else if catalog.convert_paths_where(out_ty, |t| targets.iter().all(|u| catalog.accepts(t, u))).is_empty() {
                return refuse(format!("no connected engine converts {out_ty} to {}", targets.join(" and ")));
            }
        }
        if feeds {
            let others = self.convert_to_types(catalog, &from.node, Some(to));
            match self.convert_from_type(catalog, &from.node) {
                None if !catalog.converts_to(in_ty) => return refuse(format!("no connected engine converts anything to {in_ty}")),
                None => {}
                Some(t) => {
                    let fits = |x: &str| catalog.accepts(x, in_ty) && others.iter().all(|u| catalog.accepts(x, u));
                    if catalog.convert_paths_where(&t, fits).is_empty() {
                        let more = if others.is_empty() { String::new() } else { format!(" (and to {}, which it feeds too)", others.join(", ")) };
                        return refuse(format!("no connected engine converts {t} to {in_ty}{more}"));
                    }
                }
            }
        }
        Ok(())
    }

    /// The pin problem of a wire out of a Convert, if any: the path it keeps has gone.
    pub(crate) fn convert_pin_problem(&self, catalog: &SchemaReply, id: &NodeId) -> Option<String> {
        if self.convert_pin_fits(catalog, id) != Some(false) {
            return None;
        }
        let pin = self.convert_pin(id).unwrap_or_default().join(", ");
        let now = self.convert_path(catalog, id).map_or_else(|| "nothing fits".to_owned(), |p| format!("it runs {} instead", convert::label(&p)));
        Some(format!("Convert {id} keeps the path {pin}, which no longer fits; {now} until node.convert pins it"))
    }
}

#[cfg(test)]
mod tests {
    use plumcraft_proto::catalogue::{NodeDef, Port, Socket};
    use plumcraft_proto::convert::Converts;

    use super::*;

    fn def(id: &str, ins: &[(&str, &str)], outs: &[(&str, &str)], converts: Option<(&str, &str)>) -> NodeDef {
        NodeDef {
            id: id.into(),
            display: id.into(),
            inputs: ins.iter().map(|(n, t)| Port { name: (*n).into(), ty: (*t).into(), socket: Some(Socket::Socket), ..Default::default() }).collect(),
            outputs: outs.iter().map(|(n, t)| Port { name: (*n).into(), ty: (*t).into(), ..Default::default() }).collect(),
            converts: converts.map(|(f, t)| Converts::new(f, t, true)).into_iter().collect(),
            ..Default::default()
        }
    }

    fn catalog() -> SchemaReply {
        SchemaReply {
            nodes: vec![
                def("bounce", &[("doc", "DOC")], &[("audio", "WAV")], Some(("DOC", "WAV"))),
                def("from_wav", &[("wav", "WAV")], &[("audio", "AUDIO")], Some(("WAV", "AUDIO"))),
                def("session", &[], &[("doc", "DOC")], None),
                def("wav_file", &[], &[("wav", "WAV")], None),
                def("gain", &[("audio", "AUDIO")], &[("audio", "AUDIO")], None),
                def("upper", &[("text", "TEXT")], &[("text", "TEXT")], None),
            ],
            ..Default::default()
        }
    }

    fn p(id: &NodeId, port: &str) -> PortRef {
        PortRef::new(id.clone(), port)
    }

    #[test]
    fn a_convert_takes_its_types_from_its_wires_and_keeps_its_path() {
        let c = catalog();
        let mut g = Graph::new();
        let session = g.add_node(c.node("session").unwrap(), [0.0, 0.0]);
        let conv = g.add_node(c.node(KIND).unwrap(), [200.0, 0.0]);
        let gain = g.add_node(c.node("gain").unwrap(), [400.0, 0.0]);
        let upper = g.add_node(c.node("upper").unwrap(), [400.0, 200.0]);
        g.connect(&c, p(&session, "doc"), p(&conv, PORT)).unwrap();
        assert!(g.convert_pin(&conv).is_none(), "one end: nothing to pin yet");
        // the input's type decides what the output may feed
        let e = g.connect(&c, p(&conv, PORT), p(&upper, "text")).unwrap_err().to_string();
        assert!(e.contains("converts DOC to TEXT"), "{e}");
        g.connect(&c, p(&conv, PORT), p(&gain, "audio")).unwrap();
        assert_eq!(g.convert_pin(&conv), Some(vec!["bounce".to_owned(), "from_wav".to_owned()]));
        assert_eq!(g.convert_path(&c, &conv).map(|p| convert::label(&p)), Some("bounce → from_wav".to_owned()));
        assert!(g.wire_problems(&c).is_empty(), "{:?}", g.wire_problems(&c));
        // a WAV now: the pin no longer fits, so it is replaced
        let wav = g.add_node(c.node("wav_file").unwrap(), [0.0, 200.0]);
        g.connect(&c, p(&wav, "wav"), p(&conv, PORT)).unwrap();
        assert_eq!(g.convert_pin(&conv), Some(vec!["from_wav".to_owned()]));
        // and it survives a save
        let back = Graph::from_json(&g.to_json().unwrap()).unwrap();
        assert_eq!(back.convert_pin(&conv), g.convert_pin(&conv));
    }

    #[test]
    fn the_output_decides_what_the_input_takes_and_converts_dont_chain() {
        let c = catalog();
        let mut g = Graph::new();
        let conv = g.add_node(c.node(KIND).unwrap(), [0.0, 0.0]);
        let conv2 = g.add_node(c.node(KIND).unwrap(), [0.0, 0.0]);
        let gain = g.add_node(c.node("gain").unwrap(), [400.0, 0.0]);
        let upper = g.add_node(c.node("upper").unwrap(), [400.0, 0.0]);
        let e = g.connect(&c, p(&conv, PORT), p(&upper, "text")).unwrap_err().to_string();
        assert!(e.contains("converts anything to TEXT"), "{e}");
        g.connect(&c, p(&conv, PORT), p(&gain, "audio")).unwrap();
        let text = g.add_node(c.node("upper").unwrap(), [0.0, 0.0]);
        let e = g.connect(&c, p(&text, "text"), p(&conv, PORT)).unwrap_err().to_string();
        assert!(e.contains("converts TEXT to AUDIO"), "{e}");
        let e = g.connect(&c, p(&conv2, PORT), p(&conv, PORT)).unwrap_err().to_string();
        assert!(e.contains("can't feed another Convert"), "{e}");
    }

    #[test]
    fn a_pin_that_has_gone_is_a_problem_and_runs_the_best_path_meanwhile() {
        let c = catalog();
        let mut g = Graph::new();
        let wav = g.add_node(c.node("wav_file").unwrap(), [0.0, 0.0]);
        let conv = g.add_node(c.node(KIND).unwrap(), [200.0, 0.0]);
        let gain = g.add_node(c.node("gain").unwrap(), [400.0, 0.0]);
        g.connect(&c, p(&wav, "wav"), p(&conv, PORT)).unwrap();
        g.connect(&c, p(&conv, PORT), p(&gain, "audio")).unwrap();
        if let Some(n) = g.nodes.get_mut(&conv) {
            n.extra.insert(PIN.into(), serde_json::json!(["gone.kind"]));
        }
        let problems = g.wire_problems(&c);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].1.contains("gone.kind") && problems[0].1.contains("from_wav"), "{problems:?}");
        assert_eq!(g.convert_path(&c, &conv).map(|p| p.len()), Some(1));
        assert!(g.repin_convert(&c, &conv, false));
        assert!(g.wire_problems(&c).is_empty());
    }
}
