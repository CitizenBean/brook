# Brook

Brook is a proposed Rust platform for processing event streams and running agents only when they add value. The name comes from a babbling brook.

Events can come from people, Home Assistant, timers, or other systems. An operator graph applies inexpensive rules, classifiers, or user-defined functions before spending tokens on an agent. Any operator can send results to another processor, one or more agents, or a sink such as a terminal, Discord, or Kafka. Agents are optional processors within that graph and can use swappable agentic harnesses. They can send messages to other agents and systems themselves. Tools support synchronous calls and asynchronous work with checkpointed context and tool-defined replies. A more integrated native Brook harness is proposed alongside other harnesses. Dynamic circular-dependency checks are required; their precise scope and retry handling remain under review.

The priorities are extensibility, clean interfaces, easy setup, and useful defaults. The design includes Kafka-like topics with interchangeable in-memory, local on-disk, and Kafka backends, and a preference for WebAssembly for portable user-defined functions.

## Current status

This repository contains an architecture draft for discussion. There is no implementation or stable API yet. The draft distinguishes agreed foundations, proposed contracts, and open decisions. It makes no performance or delivery guarantees for Brook.

Start with the [architecture draft](docs/architecture.md), including five diagrams and the [questions for review](docs/architecture.md#questions-for-review).

The [session, context and authorized-delivery proposal](docs/context-and-routing.md) develops multi-session routing, portable asynchronous resumptions and per-message sink authorization, with [bounded TLA+ checks](spec/README.md).

Editable Mermaid sources and SVG views are in [docs/diagrams](docs/diagrams). The diagrams show logical responsibilities, not a finalized crate layout or deployment topology.

Mermaid is the editable diagram source; the SVG files are matching previews rendered with Graphviz. Keep their nodes, edges, and labels synchronized when editing.
