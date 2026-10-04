# Brook

Brook is a proposed Rust platform for processing event streams and running agents only when they add value. The name comes from a babbling brook.

Events can come from people, Home Assistant, timers, or other systems. A processor graph applies inexpensive rules, classifiers, or user-defined functions before spending tokens on an agent. Routing operators can send results to another processor, one or more agents, or a sink such as a terminal, Discord, or Kafka. Agents are optional processors within that graph and can use swappable agentic harnesses. They can send messages to other agents and systems themselves. Tools support synchronous calls and asynchronous work with checkpointed context and tool-defined replies. A more integrated native Brook harness is proposed alongside other harnesses. The local runtime currently accepts DAGs only; broader dependency scope and feedback handling remain under review.

The priorities are extensibility, clean interfaces, easy setup, and useful defaults. The design includes Kafka-like topics with interchangeable in-memory, local on-disk, and Kafka backends, and a preference for WebAssembly for portable user-defined functions.

## Current status

This repository contains an architecture draft and an experimental local Rust core. There is no stable API or production delivery guarantee. The draft distinguishes agreed foundations, proposed contracts, and open decisions.

Run the fake-only two-session demo with `cargo run --locked -- demo /tmp/brook-demo-new` using a new directory. See [local core contracts, tests and limitations](docs/local-core.md). Seven-day garbage collection and real providers remain unimplemented.

Try the zero-config terminal recipe with `cargo run --locked -- processor-demo /tmp/brook-processors-new`. The [processor runtime](docs/processors.md) adds durable native processor DAGs, typed Rust adapters, routing operators and an uncertainty-aware terminal sink. [Easy setup, connector management and a live web DAG](docs/control-plane.md) describe the accepted product direction and distinguish implemented foundations from planned work.

Start with the [architecture draft](docs/architecture.md), including five diagrams and the [questions for review](docs/architecture.md#questions-for-review).

The [session, context and authorized-delivery proposal](docs/context-and-routing.md) develops multi-session routing, portable asynchronous resumptions and per-message sink authorization, with [bounded TLA+ checks](spec/README.md).

The [local reliability decisions](docs/local-reliability.md) cover durable admission, leased ownership, dead-letter recovery and bounded storage with configurable seven-day unused-record retention.

Editable Mermaid sources and SVG views are in [docs/diagrams](docs/diagrams). The diagrams show logical responsibilities, not a finalized crate layout or deployment topology.

Mermaid is the editable diagram source; the SVG files are matching previews rendered with Graphviz. Keep their nodes, edges, and labels synchronized when editing.
