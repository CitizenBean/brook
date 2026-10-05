# Brook

Brook is a Rust event-processing platform that uses inexpensive processors
before starting agents with swappable harnesses. A client is both a producer and
a sink; the terminal is the first example. Processors, agents, asynchronous work
and result delivery belong to one durable execution model.

Start with the [architecture](docs/architecture.md): ingress → graph/processors →
agent → asynchronous work → reply → sink. It describes logical routing, bounded
context, easy defaults, advanced graphs and the typed connector/control-plane
direction. Its [status and limits](docs/architecture.md#status-and-limits) separate
the intended design from the experimental downstream implementation.

- [Context and routing](docs/context-and-routing.md): session isolation, portable
  continuation returns and authorization of each sink intent.
- [Local reliability](docs/local-reliability.md): atomic admission, heartbeat
  leases, generation fencing, unknown-outcome recovery and bounded retention.
- [TLA+ models and results](spec/README.md): bounded checks and counterexamples,
  preserved as design evidence rather than implementation proof.

[Diagrams](docs/diagrams) pair editable Mermaid sources with SVG views. Keep their
nodes, edges and labels synchronized. They show intended responsibilities, not
separate required databases or a finalized crate layout.
