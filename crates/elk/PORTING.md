# Porting elk-swift to `upleft-elk`

`upleft-elk` is a line-by-line port of `vendor/elk-swift` (lukilabs/elk-swift
@ 32f8042, 1.0.2), the ELK Layered engine beautiful-mermaid-swift lays Mermaid
diagrams out with. The Swift is the specification. Output must be
**bit-identical**: every node, port, label position and every edge bend point,
compared as exact `f64`s. "Close" is a failure. Bugs are ported, not fixed.

Read the root `AGENTS.md` too; it is binding.

## Where things are

* One Rust module per Swift file. `src/org/eclipse/elk/...` mirrors
  `Sources/ElkSwift/ELK/org/eclipse/elk/...`; `src/bridge/` mirrors `Bridge/`.
  File `org_eclipse_elk_alg_layered_p3order_BarycenterHeuristic.swift` is
  module `crate::org::eclipse::elk::alg::layered::p3order::barycenter_heuristic`.
  `tools/modules.tsv` lists every Swift file with its Rust module.
* A module that still says "Not ported yet." is a stub. Some stubs define a
  placeholder type (`pub struct X; impl ILayoutProcessor for X { ... unimplemented!() }`)
  because the pipeline references it; replace the body, keep the type name and
  `new(...)` signature.
* `src/prelude.rs` re-exports what nearly every file needs
  (`use crate::prelude::*;`). `LayeredOptions::X`, `CoreOptions::X`,
  `InternalProperties::X` are the Swift property statics under their Swift names.
* `src/swift.rs`: Swift stdlib behaviour the port must reproduce (sorting,
  `min`/`max`, `Double(String)`, `Double.description`).

## Naming

* Types keep their Swift/Java names (`BKAlignedLayout`, `NNode`). Enum cases
  keep the Java constant spelling (`PortSide::NORTH`, `NodeType::LONG_EDGE`,
  `HierarchyHandling::INCLUDE_CHILDREN`) — the crate allows non-camel-case.
* Methods and functions are snake_case of the Swift name: `getLayoutProcessorConfiguration`
  → `get_layout_processor_configuration`. A Swift `static func` becomes an
  associated function (`Foo::bar_baz(...)`) or a free function in the module.
* Anything that reads or changes the layered graph takes the arena explicitly:
  `fn process(&mut self, lg: &mut LGraphArena, graph: LGraphId, monitor: &mut dyn IElkProgressMonitor)`.
  Read-only helpers take `lg: &LGraphArena`.

## The layered graph arena

`crate::org::eclipse::elk::alg::layered::graph::l_graph::LGraphArena` holds
every `LGraph`, `Layer`, `LNode`, `LPort`, `LEdge`, `LLabel` of one layout run.
Swift object references become `Copy` ids (`LGraphId`, `LayerId`, `LNodeId`,
`LPortId`, `LEdgeId`, `LLabelId`); `a === b` is `a == b`. `lg[id]` gives the
element record (`LNodeData`, …); fields are public and named after the Swift
stored properties:

| Swift | Rust |
|---|---|
| `node.position` / `getPosition()` (a `KVector` the caller then mutates) | `lg[n].position` (mutate the field) |
| `node.size`, `node.margin`, `node.padding`, `node.type`, `node.ports`, `node.labels`, `node.layer`, `node.nestedGraph`, `node.id` | `lg[n].size`, `.margin`, `.padding`, `.node_type`, `.ports`, `.labels`, `.layer`, `.nested_graph`, `.id` |
| `port.side` (assignment bypasses `setSide`) | `lg[p].side = ...` |
| `port.setSide(s)` (also recomputes the anchor) | `lg.port_set_side(p, s)` |
| `port.anchor`, `port.incomingEdges`, `port.outgoingEdges`, `port.owner`/`node` | `lg[p].anchor`, `.incoming_edges`, `.outgoing_edges`, `.owner` |
| `edge.source`/`target`/`bendPoints`/`labels` | `lg[e].source`, `.target`, `.bend_points`, `.labels` |
| `graph.layerlessNodes`, `graph.layers`, `graph.size`, `.padding`, `.offset`, `.parentNode` | `lg[g].layerless_nodes`, `.layers`, `.size`, `.padding`, `.offset`, `.parent_node` |
| `layer.nodes`, `layer.owner`/`getGraph()`, `layer.size` | `lg[l].nodes`, `.owner`, `.size` |
| `LNode(graph)`, `LPort()`, `LEdge()`, `LLabel(text)`, `Layer(graph)`, `LGraph()` | `lg.new_node(Some(g))`, `lg.new_port()`, `lg.new_edge()`, `lg.new_label(text)`, `lg.new_layer(g)`, `lg.new_graph()` (none of these add the element to any list, exactly like the Swift inits) |
| `node.setLayer(l)` / `setLayer(i, l)` | `lg.node_set_layer(n, Some(l))` / `lg.node_set_layer_at(n, i, l)` |
| `port.setNode(n)` | `lg.port_set_node(p, Some(n))` |
| `edge.setSource(p)` / `setTarget(p)` / `reverse(g, adapt)` | `lg.edge_set_source(e, Some(p))`, `lg.edge_set_target(...)`, `lg.edge_reverse(e, g, adapt)` |
| `node.getIncomingEdges()` / `getOutgoingEdges()` / `getConnectedEdges()` | `lg.node_incoming_edges(n)` etc. (new `Vec`s, like Swift's arrays) |
| `node.getPorts(side)`, `getPorts(type)`, `getPortSideView(side)` | `lg.node_ports_on_side(n, s)`, `lg.node_ports_of_type(n, t)`, `lg.node_port_side_view(n, s)` |
| `node.getGraph()`, `node.getIndex()`, `layer.getIndex()` | `lg.node_graph(n)`, `lg.node_index(n)`, `lg.layer_index(l)` |
| `port.getAbsoluteAnchor()` / `absoluteAnchor` | `lg.port_absolute_anchor(p)` |
| `edge.isSelfLoop()`, `isInLayerEdge()`, `getOther(port)`, `getOther(node)` | `lg.edge_is_self_loop(e)`, `lg.edge_is_in_layer_edge(e)`, `lg.edge_other_port(e, p)`, `lg.edge_other_node(e, n)` |
| `LGraphUtil.foo(...)` | `lg.foo(...)` (see `l_graph_util.rs`; add missing ones there only if you own it) |

Swift arrays are values: `for e in port.outgoingEdges { e.setSource(x) }`
iterates a snapshot. Clone the id list before mutating
(`for e in lg[p].outgoing_edges.clone() { ... }`); the borrow checker will
usually force this anyway, and it is the Swift semantics.

Algorithm-local classes (`NNode`, `BKAlignedLayout`, `HyperEdgeSegment`,
`SelfLoopHolder`, …) are ported as Rust structs. If they are cross-linked
object graphs, give them their own small arena (`Vec<T>` + index newtype)
inside the algorithm; if Swift shares an instance between owners, share it
(`Rc<RefCell<T>>`) — keep identity semantics wherever identity is observable.

## Properties (the part that bites)

Every element has `props: PropertyMap` — Swift's `[String: Any]`. Values are
`PropValue` (Swift `Any`) and keep their Swift dynamic type: `Int(i64)` and
`Double(f64)` are different variants, every enum has its own variant, Swift
classes are shared `Rc<RefCell<_>>` (`KVector`, `KVectorChain`, `ElkPadding`,
`ElkMargin`, `Random`), Swift arrays of graph elements are `LNodes(Rc<Vec<_>>)`
etc., and anything else goes in `PropValue::Object(Rc<dyn Any>)`
(store with `PropValue::object(Rc::new(x))`, read with `props.get_object::<T>(&P)`).

Two Swift read forms differ when the stored value has an unexpected type — and
it often does, because the JSON importer stores `"8"` as a `Double` even for
`Int` options, `"NONE"` as an `OrderingStrategy`, `"SEPARATE"` as a `String`…:

| Swift | Rust | stored value of the wrong type gives |
|---|---|---|
| `x.getProperty(P) as? T` (also `as? T ?? d`) | `x.props.get_as::<T>(&P)` (`.unwrap_or(d)`) | `None` |
| `let v: T? = x.getProperty(P)`, `if let v: T = x.getProperty(P)`, `x.getProperty(P) ?? d` where the context types it | `x.props.get_typed::<T>(&P)` | `P`'s default cast to `T` |
| `x.getProperty(P)` as `Any?` (e.g. `!= nil`, passed on) | `x.props.get(&P)` | the value |
| `x.hasProperty(P)` | `x.props.has(&P)` | |
| `x.setProperty(P, v)` | `x.props.set(&P, v)` (`v: impl Into<PropValue>`) | |
| `x.setProperty(P, optional)` (nil removes) | `x.props.set_opt(&P, opt_value)` | |
| `x.copyProperties(y)` | `x.props.copy_properties(&y.props)` (clone `y.props` first if both live in `lg`) | |

Decide the form from the Swift text, never from what "should" happen. Mirror
the value's Swift literal type exactly when setting: `setProperty(P, 5)` stores
an `Int` (`5i64`), `setProperty(P, 5.0)` a `Double`. A property's default
belongs to the `Property` object: `LayeredOptions::NODE_SIZE_MINIMUM` has none,
`CoreOptions::NODE_SIZE_MINIMUM` defaults to `KVector()` — use the same object
the Swift uses. Properties declared locally in a Swift file (`_Keys`, `static
let SOME_KEY = Property<...>("id")`) become `pub static` items in that module
using the generated key `keys::...` (every id appearing in the Swift source is
already in `graph/properties/keys.rs`).

Mutating a class-typed property value in place (Swift: `let jp =
edge.getProperty(JUNCTION_POINTS) as? KVectorChain; jp.add(...)`) is
`jp.borrow_mut().add(...)` on the `Rc` you got back — the stored value changes
too, as in Swift.

## KVector, KVectorChain and aliasing

Swift's `KVector` and `KVectorChain` are classes; the port uses `Copy`/`Clone`
values inside element records (`position`, `size`, `anchor`, `bend_points`).
This is exact as long as Swift never stores one vector object in two places
and then mutates it through one of them. Whenever the Swift assigns an existing
`KVector`/`KVectorChain` reference into a second location (`a.position = b`,
`setPosition(v)`, `setProperty(P, v)` with a vector that is also a field,
`chain.add(v)` where `v` is kept elsewhere) check whether either alias is
mutated or read later. If it is, reproduce the sharing (an `Rc<RefCell<_>>`,
or an explicit write-through) and comment why. One known case:
`LGraphUtil.createExternalPortDummy` makes an external port dummy's
`PORT_ANCHOR` property the same object as its port's `position`; read that
property with `lg.node_port_anchor(n)`, never `get_as::<KVectorRef>`.

## Swift semantics checklist

* **Sorting.** Swift's `sort`/`sorted` is a specific stable merge sort; ELK
  comparators are not always consistent, so use `swift::sort_by(&mut v, |a, b|
  less)` / `swift::sorted_by(iter, less)` with the exact Swift predicate
  (`areInIncreasingOrder`), never `slice::sort*`.
* **min/max.** `min(a, b)` is `b < a ? b : a`, `max(a, b)` is `b >= a ? b : a`:
  use `swift::min`/`swift::max` (and `min_of`/`max_of` for 3+ arguments,
  `seq_min`/`seq_max`/`seq_min_by`/`seq_max_by` for `Sequence.min()/max()`).
  `f64::min/max` differ on NaN and signed zero.
* **Floating point.** Keep every expression's operand order and grouping;
  `a / 2` stays a division. `abs`, `sqrt`, `floor`, `ceil`, `rounded()`
  (= `f64::round`), `sin`/`cos`/`atan2`/`pow` (= `powf`, never `powi`) map
  directly.
* **Integers.** Swift `Int` is 64-bit; `Int(x)` truncates; `/` and `%`
  truncate like Rust. Swift traps on overflow; don't add wrapping unless Swift
  uses `&+` etc.
* **Release-build Swift.** Downright and the oracle are `-O` builds:
  `assert(...)` and `assertionFailure(...)` do **nothing** — keep going exactly
  as the code after them does. `precondition`, `fatalError`, force unwraps
  (`!`), `as!`, out-of-range array indexing and integer overflow trap: port
  them as panics (indexing/`unwrap()`/`expect()` do that naturally).
* **Hash-ordered collections.** Swift `Set`/`Dictionary` iterate in an order
  that depends on a per-process hash seed, and for keys hashed by
  `ObjectIdentifier` (all ELK classes) on heap addresses — it is effectively
  random per run. Lookups are fine (`HashMap`/`HashSet`, or a dense `Vec`
  indexed by id). For every *iteration* over a Swift `Set`/`Dictionary`, decide
  whether the order can affect the result:
  * If it cannot (commutative accumulation, membership tests), iterate in any
    order.
  * If it can, the Swift output is nondeterministic. Use the order the
    original Java ELK used when it had a `LinkedHashSet`/`LinkedHashMap`
    (insertion order); otherwise insertion order. Mark the site with a
    `// NONDETERMINISTIC IN SWIFT:` comment explaining the choice, and list it
    in your report. (Known: `NetworkSimplex.treeEdges` — insertion order,
    which is also the most frequent Swift outcome.)
  * `Set<PortSide>`-style enum sets are `EnumSet<E>` (bit sets, ordinal
    order); Swift iterates them in seed order, so the same rule applies.
* **Identity-keyed maps** (`[ObjectIdentifier: X]`, `[LNode: X]`) become
  `HashMap<LNodeId, X>` or a `Vec` indexed by the id.
* **`ArrayDeque`** (elk-swift's FIFO) → `VecDeque` (same order).
* **`String(describing:)`/`"\(x)"`** of doubles → `swift::describe_double`.

## Processors and phases

`core::alg::i_layout_processor::ILayoutProcessor` (`process`, `name`, and
`is_hierarchy_aware` — true only for `LayerSweepCrossingMinimizer`).
Phases also implement `ILayoutPhase`, whose
`get_layout_processor_configuration` is only called for the types listed in
`AlgorithmAssembler.swift`'s `AnyLayoutPhaseBox` extensions; every other phase
keeps the default (`None`) even if its Swift class defines the method.
Processor instances are created fresh per `build`, but the components of one
graph share one processor list — instance fields persist from one component
to the next exactly as in Swift, so port instance state faithfully (including
what is *not* reset).

Progress monitors: port `begin`/`done`/`subTask` calls as written
(`monitor.begin("...", 1.0)`, `if let Some(mut sub) = monitor.sub_task(w) {
...(sub.as_mut()) }`); they only matter for `is_running`/`is_canceled`.

## What is reachable

Only the defaults of the phase strategies can be selected through the JSON
bridge (see `layered_phases.rs`): `GreedyCycleBreaker`,
`NetworkSimplexLayerer`, `LayerSweepCrossingMinimizer(BARYCENTER)`,
`BKNodePlacer`, `OrthogonalEdgeRouter` (and `PolylineEdgeRouter` if a graph
asks for `POLYLINE`). The other phase implementations are not ported. Within a
reachable class, port everything, including branches beautiful-mermaid's
options don't take. Mermaid's options are listed in
`vendor/beautiful-mermaid-swift/Sources/BeautifulMermaidSwift/Mermaid/src_layout.swift`
(`_buildElkGraph`, `_buildElkGraphNoCrossEdges`), `src_class_layout.swift`,
`src_er_layout.swift`.

## Tests

Port the relevant tests from `vendor/elk-swift/Tests/ElkSwiftTests` into Rust
(`#[cfg(test)] mod tests` in the module, or `crates/elk/tests/*.rs` using the
shared helpers in `crates/elk/tests/common/`). Same assertions, same numbers.

## Checking against Swift

* `just oracle` builds `target/oracle/release/downright-oracle`; its `elk`
  command lays out an ELK JSON graph (`downright-oracle elk in.json out.json`).
  `upleft-oracle elk ...` is the Rust side; `just conform --suite elk` compares.
* `UPLEFT_ELK_TRACE=1` makes the Rust layout print the layered graph after
  every processor to stderr. An instrumentable copy of elk-swift (never edit
  `vendor/`) with the same trace (`ELKLAB_TRACE=1`) is built by
  `tools/elklab.sh`; diff the two traces to find the first diverging
  processor. Copy the lab if you want to add your own instrumentation.
