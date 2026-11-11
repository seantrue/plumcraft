# 23. Named ports: a graph's own parameters

**Proposal, 2026-11-10. Slices 1 and 2 built the same day** (the maintainer: "Slices 1 and 2", with this note's picks for the open questions: no `.` in names, one name per port, no named outputs, no required parameters). Slice 3, the readers elsewhere, is not. Written from the research software's side, after its own graph runner gained `run --set NODE.PORT=VALUE` the same day (uncommitted there).

The maintainer, 2026-11-10:

- "Do we have a way to bind command line arguments to graph ports (input files, output files, etc)?"
- "Can we write to a port by label?"
- "I think we need to be able to assign a port a "net-list" like label, even if there is no connection to it. That sounds like a plumcraft change ..."

## 1. Today

- **A value reaches a port from outside only by node id and port name.**
  - The research software's graph runner: `run g.json --set picture.path=in.jpg`. The node may also be named by its title, when only one node has it.
  - PlumCraft: `plumcraft-cli run` takes a workflow and `--watch NODE.PORT` (`apps/plumcraft-cli/src/main.rs`). It has no way to set a value. Whether the research software's PlumCraft application's `run` has one was not checked.
  - Node ids are the editor's (`n12`), and titles are free text that change when someone tidies a graph. Neither is something a script should depend on.
- **Net labels can't do it** (`crates/graph/src/nets.rs`). Its own header rules it out twice:
  - "every net has at least one wire (the net goes when its last wire does)", so an unwired port can't hold one;
  - "Labels are only how the graph looks. The graph is the same with or without them", so no runner reads them.
- **A group already has the idea, for its own surface.** `Promoted` (`crates/graph/src/groups.rs:50`) names an inner input on the group: `port`, `label` (unique in the group), `doc`, and an optional narrower `min`/`max`. A collapsed group shows it as a widget.
- **The top level has no surface.** A whole graph has no list of what it takes or gives.

## 2. The proposal

**Give the top-level graph the surface a group has: a list of named ports.** A named port says "a value from outside lands here". It is a property of the port, with or without a wire elsewhere on the node.

- **Storage:** `Graph::named`, a list at the top level beside `nets`, not written when empty:

  ```json
  "named": [
    {"name": "input",  "port": {"node": "n1", "port": "path"}, "doc": "The picture to render"},
    {"name": "output", "port": {"node": "n9", "port": "path"}},
    {"name": "scales", "port": {"node": "n3", "port": "scales"}}
  ]
  ```

  - The entry is `Promoted`'s shape with `label` called `name`. Reusing the struct itself is the smaller change; a field alias would keep one type for both. `min`/`max` come along and mean the same.
  - Older editors keep the field without understanding it: `Graph` flattens unknown fields into `extra` and writes them back.
- **What it means:**
  - A runner given `name = value` stores the value as that port's widget value, read as the port's type, and then runs the graph as it is.
  - **The name is not part of what runs.** The run request, the memo identity and staleness read values, as now. Setting `scales` to 2 by name and typing 2 into the widget are the same run.
  - **The value is.** It changes the result exactly as editing the widget does.
- **Rules**, kept after every edit and on load, as `prune_nets` does for nets (a file's names are hostile input; a bad entry is dropped, the graph kept):
  - names are trimmed, 1 to 40 characters, no control characters, no `=`; unique among the graph's named ports;
  - the port exists on the node's current definition and holds a value (it has a widget), and is not wired: a wire would win, so the name would do nothing;
  - one port has at most one name;
  - the entry goes when its node goes, or when a changed choice removes its sub-port (as promotions do, `SetOutcome::unpromoted`);
  - wiring a named port is refused with a message naming it, not silently unnamed. The designer removes the name first.
- **Groups:** nothing new. A group's `promoted` list is already this, scoped to the group. A named port at the top level may be a group's promoted parameter, addressed by the port it promotes.

## 3. In the editor

- **Draw it as a net label with no wire:** a stub and the name on the input pin, in the wire type's colour. It moves with the node, as net labels do.
- **Set it from the port's menu:** "Name this port…", "Rename…", "Remove name". The same place net labels are made.
- **List them:** a "Parameters" section in the graph's own properties, in order, with each doc line and a jump to its node. Order matters only for display and for `--help` (section 4).
- **Commands**, so the control channel and tests reach it: `graph.port.name {node, port, name, doc}`, `graph.port.unname {name}`, `graph.port.rename {name, to}`, `graph.port.reorder`.

## 4. On the command line

- **`plumcraft-cli run WORKFLOW --set NAME=VALUE`**, repeatable, and the same in each application's `run`.
  - A bare `NAME` is looked up in `named`. `NODE.PORT=VALUE` stays as the escape hatch for a port nobody named.
  - A value is read as the port's type: INT, FLOAT, BOOL (`true`/`false`), an ENUM's choice, STRING as written; any other type as JSON.
  - An unknown name, a value of the wrong type or a choice that isn't offered exits before the run, naming the assignment and listing what the graph has.
- **`plumcraft-cli run WORKFLOW --params`** prints the surface: each name, type, default (the stored value), range or choices, and doc. This is the graph's `--help`.
- **Paths:** `anchor_workflow` (`crates/session/src/paths.rs`) makes a file's relative path inputs absolute against the workflow's folder. A path given with `--set` is the caller's, so it is relative to the current directory. Apply `--set` after anchoring, and make its relative paths absolute against the current directory.
- **The research software's graph runner** reads the same field: its `run --set NAME=VALUE` looks in `named` first. A few lines there, once the format here is settled.

## 5. What it does not change

- **No interface change.** Engines get a graph with values in it, as now; `docs/interface.md` and `schema/protocol2.json` are untouched. Names are resolved by whoever builds the run request.
- **No change to any existing graph.** A file without `named` behaves as today.
- **Net labels stay as they are**, a way of drawing a wire.

## 6. Slices

1. **Model and command line.** `Graph::named`, its rules and pruning, hostile-input tests beside the nets ones; `--set` and `--params` in `plumcraft-cli run`. Testable with a hand-edited file before any canvas work.
2. **Editor.** The pin stub, the port menu, the Parameters list, the commands.
3. **Readers elsewhere.** The research software's PlumCraft application's `run`, and its own graph runner.

## 7. Open questions

Each with what this note would pick.

- **Array and other socket inputs.** An unwired `image` socket named `input`, fed a file from the command line, is the obvious next wish. It needs the runner to load a file into a socket, which no runner does today. *Pick: not in this change.* Name the Load node's `path` instead. Revisit with Convert (note 20), which already knows how a file becomes a core type.
- **One name on several ports.** One `scales` driving two scale nodes saves a constant node and two wires. It also makes "what is this parameter's default" ambiguous when the ports hold different values. *Pick: one name, one port.* A graph that needs fan-out uses a constant node and names that.
- **Outputs.** Naming an output port would give `--watch result` and a declared "this is what the graph produces". It fits the same list, but an output takes no value, so `--set` on it is an error and the rules about wires don't apply. *Pick: slice 2 or later, as a `dir` field on the entry, absent meaning input.*
- **A name that matches a node id or title.** `--set picture.path=x` and a port named `picture.path` could both exist. *Pick: forbid `.` in names too,* so a bare name and `NODE.PORT` can never be confused.
- **Required parameters.** A named port whose stored value is empty (a Load node with no path) could make `run` refuse to start without `--set`. *Pick: not yet;* the node's own error already says what is missing.
- **Scripts and stored commands.** A stored command made from a whole graph could take its parameters from `named`, as one made from a group takes them from `promoted`. Not looked into.

## 8. Not verified

- Whether the research software's PlumCraft application's `run` already has a value override.
- How the canvas draws a label on a pin with no wire; net labels are drawn from wires, so the stub may need its own pass.
- Whether a third-party reader of workflow files treats an unknown top-level field as an error. PlumCraft's own readers keep it.
