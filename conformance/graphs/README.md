# The shared graph vectors

The corpus `docs/interface.md` section 7.6 names: small graphs of the public engines' building blocks, each with the item structure its rule gives at named outputs. They hold every executor to the same list rules (rule 2, lists and nested lists; rule 3, slices). Started for nested lists (research note 27, section 7).

## A vector

One JSON file a vector, named `NN-what-it-shows.json`:

| Field | Meaning |
|---|---|
| `name` | One line: what the rule does here |
| `about` | The graph in words, and what it gives under each rule |
| `needs` | The capabilities whose rule `expect` describes (today only `lists.nested`); empty or absent: every engine |
| `graph` | A workflow (section 7.6): `{version, nodes: {id: {kind, engine, values}}, wires: [{from, to, slice?}]}`. Every node names its `engine`, so a kind more than one engine offers is run where the vector says |
| `expect` | `{"node.port": {shape, values?}}`: what the rule gives at those outputs |
| `today` | The same, under the rule before `needs`: required when `needs` isn't empty |

**Shape** is the item structure: a list's length, then its items' common shape; a value that isn't a list has the shape `[]`. So `[3, 2]` is three lists of two, and `[6]` one flat list of six. A list whose items differ in shape is ragged and matches no shape. **Values**, when given, are the items as plain JSON, lists as arrays, and must have the vector's shape.

## Running them

`crates/coordinator/tests/vectors.rs` checks each vector's form (its kinds are in the engines' catalogues, its expectations name real outputs) and runs it through the coordinator on the text, array and test (`ports`) engines, in process. A vector that `needs` a capability is held to `expect` once every engine its nodes name lists that capability; until then it is held to `today`, and it fails if it already gives `expect`, so a marker is never left behind.

## The vectors

| File | Shows | Needs |
|---|---|---|
| `01-runs-keep-their-lists.json` | A step run once per item keeps each run's list: 3 × 2 | `lists.nested` |
| `02-index-slice-inside-each-run.json` | An index slice takes one item of each run's list | `lists.nested` |
| `03a-range-slice-inside-each-run.json` | A range slice cuts each run's list | `lists.nested` |
| `03b-step-slice-inside-each-run.json` | A stepped slice applies inside each run | `lists.nested` |
| `04-whole-list-port-once-per-run.json` | A whole-list port runs once per outer item | `lists.nested` |
| `05-item-and-whole-list-inputs.json` | A step with an item input and a whole-list input, both from one list: zipped on the outer axis (the test engine's Item and List) | `lists.nested` |
| `06a-two-ranges-product.json` | Two ranges on different axes: every combination | `lists.nested` |
| `06b-one-range-twice-zips.json` | One range into two inputs zips | `lists.nested` |
| `07-three-levels.json` | A range after a nested list adds a level: 3 × 2 × 2 | `lists.nested` |
| `08-across-an-engine-border.json` | Vector 2 across an engine border | `lists.nested` |
| `09-one-item-a-run-unchanged.json` | A step making one item a run is unchanged | none |
| `10-b164-range-slice-whole-list.json` | The wiring of B164 as note 27 first read it: a range of 5, a step of 2 a run, the slice `1`, a step of 3 a run, a whole-list step | `lists.nested` |
| `11-whole-list-input-with-a-sweep-of-its-own.json` | The wiring of B164 as the report's graph has it: the mask step run once per scale and once per run (a product of 15 over two sweeps, through a variadic input's slots), and a step that takes the masks and the scales whole runs once per run with its 3 | `lists.nested` |
