# 27. Nested lists: one result per value when a step returns a list per run

**Research, 2026-11-24, for B164. Nothing is built; the decisions in section 9 come first.**

The maintainer, 2026-11-24 (B164): "Node driven by range does not generate multiple results on output", then, on the options: "Option 1, do the option 3 message first".

Paths are in this repository unless they name another codebase. "The research software" is the executor PlumCraft's floor is measured against; "an application's own engine" is an engine an application compiles into the native shell, written to the same rules. Both are the maintainer's other repositories and are cited by what they contain, not by path.

## 1. Summary

- **Today every executor flattens.** When a node runs once per item of a list and each run returns a list, the runs' lists are joined into one flat list; a wire's slice then indexes that flat list. The rule is the same, nearly word for word, in the Rust SDK (`crates/engine/src/exec.rs:735`, "one that is itself a list per run is a new, flat list"), the Python SDK (`python/src/plumcraft_engine/exec.py:487`), the research software's executor and an application's own engine.
- **So a range in front of such a step gives one result.** A range of 5 values into a step that returns 2 pictures per run gives one list of 10; a slice `1` on the next wire takes item 1 of the 10, the first run's second picture, and everything after it has one value. Slicing every second item instead (`1::2`) gives 5, but a later step that takes a whole list of 3 then receives 15 and fails ("has 15 arrays for 3 scales").
- **The fix decided is nested lists.** A node run once per item keeps each run's list as one item: the outer level runs along the range, a slice applies inside each run's list, and every port maps over the outer level. The graph above then gives 5 results at every step and 5 at the result.
- **It breaks almost nothing that exists.** Of 394 graphs read (section 3), one relies on flattening: the graph that showed the fault.
- **The wire format already carries it.** `PortValue::List` holds `PortValue`s, so a list of lists crosses the interface today (`crates/proto/src/data.rs:179`); bindings read nested lists to a depth of 8 (`crates/engine/src/bind.rs`). What doesn't cross is a list's axes (section 4).
- **The interim message's field becomes unnecessary.** Under nested lists the per-run counts are in the value itself, so `run.items` (proposed for the interim message) would be dead once this ships. Section 4 says what that means for the interim message.
- **Order:** the rule and the shared vectors first, then the two SDKs, the coordinator and core, then an application's own engine and the research software together (section 6).

## 2. The rule today, and the case that fails

### 2.1 What the interface says

`docs/interface.md` 7.6 is normative for every executor:

- Rule 2, **Lists**: "a `list: true` port takes or makes whole lists. A list into a single-item port maps the node over the list. Lists zip along a shared axis and form a product over different axes; a length mismatch on one axis is a node error."
- Rule 3, **Wire slices**: "A range keeps a list a list, an index takes one item … A wire between engines is sliced by the coordinator, since a binding replaces the wire."
- Rule 9, **Lineage**: "every output carries, per item, what made it."

The rule says nothing about what a mapped node's output is when each run makes a list. Every executor answers it the same way: join the runs' lists flat.

### 2.2 The executors

| Executor | Mapping plan | Joining the runs | Where |
|---|---|---|---|
| Rust SDK | `mapping_plan`: the union of the inputs' axes, row-major, at most `MAX_RUNS` (100,000) runs | an output of one item a run keeps the runs' axes; one that is a list a run becomes a new flat list, each item labelled "run label · item label" | `crates/engine/src/exec.rs:623`, `:735-765` |
| Python SDK | the same | the same, with the same labels | `python/src/plumcraft_engine/exec.py:425-510` |
| The research software | `mapping_plan` | `_flatten` the runs' outputs; the axes kept only when each run made one item; per output, the items each run made (`counts`), kept in the cache and used for lineage (`sub`) | its `graph/executor.py`, `_call` and `_flatten` |
| An application's own engine | a port of the research software's executor, "the same rules in the same order" | the same flattening and `counts` | its `executor.rs`, `call` |

A wire's slice is applied after the join, to the flat list: in each engine's input resolution, and at an engine border by the coordinator (`crates/coordinator/src/runner.rs:127-136`).

### 2.3 The case that fails

A style graph of the research software, described generically:

1. A **range** of 5 values (0.1 to 1.0) feeds a numeric setting of a **layer step** that takes one value.
2. The layer step returns **2 pictures per run** on a list output.
3. The next wire carries the slice **`1`**, the second picture, into a split.
4. Further on, a **mask step** returns 3 masks per run (one per scale) into a **combine step** whose input takes the whole list of 3.

Measured on the application (dev `3403749`, B164's report):

| Step | Items today | Items under nested lists |
|---|---|---|
| The range | 5 | 5 |
| The layer step | 10 (5 runs of 2, flat) | 5 lists of 2 |
| After the slice `1` | 1 (the first run's second picture) | 5 |
| The mask step | 3 | 5 lists of 3 |
| The combine step (whole list of 3) | 1 | 5 (once per run, each with its 3) |
| The final result | 1 | 5 |

The layer step did run 5 times: the second pictures of the 10 items differ with the threshold (red means 0.955385, 0.955781, 0.95623, 0.956624, 0.957003), the first pictures are equal.

## 3. What existing graphs change meaning

### 3.1 Which wirings depend on flattening

A graph changes meaning only where a **joined list** exists: the list output of a node that ran more than once. Three wirings read one:

- **(a) a slice over a joined list**: today it indexes the flat list; under nesting it applies inside each run;
- **(b) a joined list into a whole-list port**: today the port gets every run's items as one list; under nesting it gets each run's list and runs once per run;
- **(c) a joined list into a single-item port**: today the next node maps over all items (10 runs); under nesting it maps over both levels (the same 10 runs), and its output becomes a list of lists (the same items, grouped).

A node fed by a mapped node whose output is one item a run (the common case: a range into a filter) is unchanged: it was a list over the runs' axes and stays one.

### 3.2 Counts

Read only, with the research software's catalogue for each port's `list` flag, propagating "mapped" from every list source (a range, or any list output) through ports that take one item; an index slice takes one item and stops the propagation. Kinds the catalogue lacks are counted as unknown and treated as not mapping.

| Collection | Graphs | With a range or vary | (a) slice over a joined list | (b) into a whole-list port | (c) mapped again | Graphs affected |
|---|---|---|---|---|---|---|
| This repository (JSON graphs and fixtures) | 32 | 0 | 0 | 0 | 0 | 0 |
| The application's style collection | 56 | 1 | 1 | 0 | 0 | **1**: the graph of B164 |
| The application's own bundled styles | 39 | 0 | 0 | 0 | 0 | 0 |
| The research software's workflows, examples and tests | 299 | 17 | 0 | 0 | 0 | 0 |

The style collection has 9 kinds the catalogue lacks (another engine's arrays and file nodes, and two edge steps), in graphs with no range. The 17 ranged workflows of the research software all feed steps that return one item per run. The Rust and Python SDKs' tests build their graphs in code; one test pins today's flattening (`python/tests/test_vary.py::test_a_node_returning_lists_per_run_gives_one_flat_labelled_list`) and would change with the rule; no Rust test covers it (`crates/engine/tests/vary.rs` tests products and labels of one-item runs).

### 3.3 Gating

There is no graph format version beyond `"version": 1`, and no capability for list semantics. With one graph affected, and that one the reason for the change, applying the new rule to every graph is cheap. A capability (`lists.nested`) would still let a coordinator tell an old engine from a new one across a border (section 4.1).

## 4. The wire format, memo identity, lineage, held values

### 4.1 Values

- **Nested values already cross.** `PortValue::List { items: Vec<PortValue> }` nests (`crates/proto/src/data.rs:179`). A bound list is read item by item to depth 8 (`crates/engine/src/bind.rs`). An SDK turns `Data::List` into `PortValue::List` recursively (`crates/engine/src/data.rs:178`). No message field is needed for nesting itself.
- **Axes don't cross.** `crate::List` has `axes` and `labels` (`crates/engine/src/lib.rs:64`); `PortValue::List` has neither. Across an engine border a list arrives as one anonymous axis. This is an existing gap, independent of nesting: two ranges in different engines meeting at one node zip (the same anonymous axis) where inside one engine they would form a product. Nesting makes it matter more, because a nested level must be told from its parent. The smallest addition is an optional `axes: [{name, n}]` on `PortValue::List`, under a capability.
- **A tuple is also a list.** Vary Range 2D's points travel as a list of numbers (`exec.rs:782`, a tuple `(x, y)`). Under nesting, "a list of lists" must not be mistaken for a run level. Either tuples get their own form, or a run level is marked by its axis (the addition above).

### 4.2 Memo identity

- A plan key names its wired inputs as the upstream key, the port and the slice (`docs/interface.md` 14; an application's engine at its `executor.rs:369`). The text of a slice doesn't change, but what it picks does. **Every engine's salt must change** with the rule, so no flat result is reused as a nested one.
- A content identity of a list is the list of its items' identities, recursively (`crates/engine/src/exec.rs:202`, `bind.rs:50`). Nested values are already distinct from their flat form.

### 4.3 Lineage, held values, handles

- Lineage records per item which item of each list a run took (`pick`) and which item of its own list it is (`sub`), in the research software and an application's engine. Under nesting `sub` is the inner index and the run is the outer one: the information is there, only its shape changes.
- Held values (rule 11) are named per `node.port`; a held list is one id. Unchanged.
- Handles name blobs, not lists. Unchanged.

### 4.4 What this means for `run.items` and the interim message

`run.items` (proposed for the interim message: a node's run count and each run's item count) is exactly the information a nested value carries. **Once nested lists ship, `run.items` is unnecessary**, and the situation the message describes (a slice picking from a joined list) no longer arises. Building the field means changes in both SDKs, the core and an application's engine for a message with a lifetime of one release cycle. The interim message is worth having only if nested lists are more than a release away; if so, a cheaper interim is the core's own note from what it already has: "takes item 1 of 10" (the total, from the display) without the runs, plus a pointer to B164. Decision 1.

## 5. Viewers and shells

- **A display shows one level.** `Display.items` is a count and `index` one integer (`crates/proto/src/display.rs:237-239`); `WatchRequest.index` is one integer (`:62`); the SDK's watched display picks item `index` of the top list (`exec.rs:1159`).
- **The steppers are one level.** The Apple shells' viewer bar steps "item i of n", with names from the sheet's labels (`native/apple/PlumCraftFFI/Sources/PlumCraftShell/Viewers.swift:123`, `:182`); Windows the same (`native/windows/src/PlumCraft.Models/ListNames.cs:34`); egui the Watched panel's stepper (`crates/ui/src/watched.rs:314`, `:358`).
- **What nesting needs:** `index` as a path (`[2, 1]`: run 3, item 2), `items` per level, and labels per level (the run's: "threshold 0.3"; the item's: "edges"). The stepper shows two lines, or one stepper over the outer level with the inner items side by side: a design choice (decision 6). The sheet (`viewer.sheet`) becomes a grid, runs by rows.
- **Faces:** a face preview shows one picture and a count (`items`); "5 × 2" reads better than "10".
- **Conformance:** the cases that step items ("a watched list shows one item with a sheet of labels", B159's zoom through items) gain a nested variant.

## 6. Codebases and an order of slices

Each slice ships alone and is tested alone.

| # | Slice | Where | Ships when |
|---|---|---|---|
| 1 | **The rule and its vectors.** 7.6 rule 2 and 3 rewritten (section 2 above, with the decisions); shared vectors of graphs with expected `order`, item shapes and slices (section 7). The corpus `conformance/graphs/` that 7.6 names doesn't exist today: the vectors start it | this repository | the maintainer accepts the rule |
| 2 | **The Rust SDK.** `call` keeps each run's list as one item when a run returns a list; slices index inside a run; a whole-list port maps over the outer level; salt changed; a test of the new shape added (none pins the old one in Rust) | this repository | the vectors pass |
| 3 | **The Python SDK**, the same | this repository | the vectors pass on both SDKs |
| 4 | **The coordinator and core.** The border's slice applies inside each run (`runner.rs:127`); the display path and `run.item` take an index path; the scene's item count reads "5 × 2" | this repository | a cross-engine vector passes |
| 5 | **The shells.** A two-level stepper and sheet in egui, the Apple shells and Windows | this repository | the conformance variants pass on each |
| 6 | **An application's own engine and the research software, together.** Both executors' `call` and slicing, lineage's shape, salts | the maintainer's other repositories | their own executor tests and the vectors pass |

**The floor.** PlumCraft must never support less than the research software; supporting more is allowed. Slices 2 to 5 make PlumCraft's own engines give 5 results where the research software gives 1: more, not less. The risk is the other direction: **the same graph file gives different results in the research software and in an application's engine** while slice 6 is pending, and an application's engine runs the maintainer's graphs. So slice 6 changes an application's engine and the research software **together**, in one release, and the application can ship nested lists only after it. Changing the research software first would leave every application's engine behind it; last, would leave the floor behind the applications.

## 7. Tests

- **Shared vectors** (slice 1), each a graph, its expected item shape per output and its order, run by both SDKs, the coordinator, and through the conformance runner by the shells:
  1. a range of 3 into a step returning 2 per run: shape `3 × 2`;
  2. the same with an index slice `1` after it: `3`;
  3. with a range slice `0:2`: `3 × 2`, and `::2`: `3 × 1`;
  4. a whole-list port after it: runs 3 times;
  5. a step with one per-item input and one whole-list input, both from mapped nodes on the same range: zips on the outer axis;
  6. two ranges into one step, different axes: product `a × b`; same axis: zip;
  7. a second range downstream of a nested list: three levels, within the depth limit;
  8. the same graphs across an engine border (the coordinator's slicing);
  9. a step returning one item per run: unchanged from today.
- **The maintainer's wiring**, as a private conformance case of the application: a range of 5 into a layer step's dotted setting, the slice `1`, the mask and combine steps, the result. It asserts 5 items on the driven step's sliced output and 5 at the result. It fails today (1 and 1).
- **The pinned tests** of today's flattening are rewritten to the new shape, not deleted.

## 8. Risks and what is not known

- **The run count grows.** A list of lists mapped again runs the product of both levels, as today, but deeper nesting (a range after a range after a per-run list) can reach `MAX_RUNS` (100,000) sooner. Unchanged limit, more ways to reach it.
- **Axes across engines** (section 4.1) is an existing fault, made more visible. Without it a nested list crossing a border loses which level is which.
- **Tuples versus levels** (section 4.1) must be settled before slice 2.
- **Memo**: without a salt change an old flat result could be read as a nested one.
- **Not determined:** how the research software's own UI (outside PlumCraft) shows lists of lists, if at all; whether any of the research software's own nodes return a list per run that is meant to be flat (a step that splits a picture into tiles, say, then is driven by a range); the 9 kinds the catalogue lacks in the style collection; graphs outside the four collections read.

## 9. Decisions for the maintainer

1. **The interim message.**
   - A: build `run.items` and the full message ("item 1 of 10, 5 runs of 2 each").
   - B: the core's note without run counts ("takes item 1 of 10"), from what it already has, pointing to B164.
   - C: no interim message; go straight to nested lists.
   - **Recommended: B** if nested lists are more than a release away, else C. `run.items` is made unnecessary by nesting (section 4.4).
2. **Gating.**
   - A: the new rule for every graph.
   - B: only for graphs that opt in (a graph field).
   - C: a capability `lists.nested` on engines, the rule for every graph, the coordinator refusing to mix old and new engines across a border.
   - **Recommended: C**: one graph is affected (section 3.2), and the capability protects cross-engine runs during the rollout.
3. **Slices.**
   - A: a slice always applies to the innermost level.
   - B: to the level the wire's source made (inside each run for a mapped step's list output; the outer level for a range).
   - C: a slice syntax that names the level.
   - **Recommended: B**: a slice on a wire means what it means without the range, which is the maintainer's expectation.
4. **Whole-list ports.**
   - A: map over the outer level, each run's list whole.
   - B: receive the whole nested value.
   - **Recommended: A**, which makes the combine step of section 2.3 work unchanged.
5. **Joining back.**
   - A: an explicit Flatten building block (beside Zip, Outer Join and Unzip, `crates/engine/src/seq.rs`) for a graph that wants today's flat list.
   - B: none.
   - **Recommended: A**: a graph that relied on flattening can get it back with one node, and none exists today.
6. **The stepper.**
   - A: two steppers, run and item.
   - B: one stepper over runs, a run's items side by side.
   - C: one flat stepper labelled "run 3 · item 2".
   - **Recommended: A** for pop-outs, C for the face's count.
7. **Axes across engines.**
   - A: add `axes` to `PortValue::List` (additive, under a capability).
   - B: keep anonymous lists across borders and document the zip.
   - **Recommended: A**, needed for nested levels to survive a border (section 4.1).
8. **Tuples.**
   - A: a tuple form of its own.
   - B: tuples stay lists; a run level is told by its axis.
   - **Recommended: B**, if decision 7 is A.
9. **Order.**
   - A: section 6's order, with an application's engine and the research software together last.
   - B: the research software first.
   - **Recommended: A**, for the floor reason in section 6.

**Decided (the maintainer, 2026-11-24):** given the nine decisions above with the recommendation for each, they answered "Decisions: 1B; 2C; 3B; 4A; 5A; 6A; 7A; 8B; 9A". That is the recommendation in every row, with decision 1 settled as B: a plain note from what the core already has, and no `run.items` field. Before the note was written they had chosen nested lists over the other three ways of fixing B164, and the order: "Option 1, do the option 3 message first".

**Assumed (the controller, 2026-11-24, the maintainer away overnight and having said "Make reasonable assumptions about what I choose if I am not available for a decision"):** slice 1's open points were taken on their recommendations. (1) A whole-list port fed three or more levels runs once per item of every outer level, the innermost list whole. (2) A coordinator refuses any run with a border between an engine that lists `lists.nested` and one that doesn't. (3) Flatten joins one level, kind `seq.flatten`. (4) Lists nest at most 8 levels. (5) The test engine gains a step with an item input and a whole-list input (`ports.mixed`), and vector 5 uses it. (6) An inner level maps only when it carries an axis; a list without one is a tuple, a value. (7) The runs of a product are one level with two axes. (8) The spec's "specified, not built" lines stay, each removed as its slice lands. One new point was met building slice 2 and settled as these imply: Flatten must take a nested list whole, so a node can say so (`flags.nested_input`); without it a whole-list port maps over the outer levels.

