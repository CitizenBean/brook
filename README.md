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

## Try the experimental implementation

This checkout contains an experimental Rust implementation. There is no stable
API or production delivery guarantee.

Run the fake-only two-session demo with `cargo run --locked -- demo /tmp/brook-demo-new` using a new directory. See [local core contracts, tests and limitations](docs/local-core.md). Seven-day garbage collection and real providers remain unimplemented.

Try the zero-config terminal recipe with `cargo run --locked -- processor-demo /tmp/brook-processors-new`. The [processor runtime](docs/processors.md) adds durable native processor DAGs, typed Rust adapters, routing operators and an uncertainty-aware terminal sink. [Easy setup, connector management and a live web DAG](docs/control-plane.md) describe the accepted product direction and distinguish implemented foundations from planned work.

[Diagrams](docs/diagrams) pair editable Mermaid sources with SVG views. Keep their
nodes, edges and labels synchronized. They show intended responsibilities, not
separate required databases or a finalized crate layout.
