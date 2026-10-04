#![doc = include_str!("../../../docs/extensibility.md")]

use brook::processing::{
    Graph, JsonShape, PipelineBuilder, Routing, Schema, TerminalIdentity, TypedProcessor,
    TypedProposal, Version,
};
use brook::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefixConfig {
    pub prefix: String,
}

impl PrefixConfig {
    /// Consumer-owned validation; Brook has no typed config hook yet.
    pub fn parse(value: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(value.clone())?;
        if config.prefix.len() > 32 {
            return Err(Error::Invalid("prefix exceeds 32 UTF-8 bytes"));
        }
        Ok(config)
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Count {
    pub processed: u64,
}

pub struct Prefix;

impl TypedProcessor for Prefix {
    type Input = String;
    type State = Count;
    type Output = String;

    fn code(&self) -> Version {
        Version::new("prefix", 1)
    }

    fn process(
        &self,
        input: String,
        state: Count,
        config: &Value,
    ) -> Result<TypedProposal<Count, String>> {
        let config = PrefixConfig::parse(config)?;
        let processed = state.processed.checked_add(1).ok_or(Error::Capacity)?;
        Ok(TypedProposal {
            reason: "configured prefix".into(),
            state: Count { processed },
            outputs: vec![config.prefix + &input],
            routing: Routing::ConfiguredNext,
        })
    }
}

/// Application-specific factory, not a general Brook registry or compiler.
pub fn prefix_graph(binding: Version, config: Value) -> Result<Graph> {
    PrefixConfig::parse(&config)?;
    let mut graph = PipelineBuilder::terminal(&TerminalIdentity::local(), binding)
        .prepend("prefix", Prefix.code(), config)?
        .build();
    let node = graph.nodes.get_mut("prefix").expect("just inserted");
    node.state_schema = Schema {
        identity: Version::new("prefix-count", 1),
        shape: JsonShape::Object,
    };
    node.initial_state = json!({"processed": 0});
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use brook::processing::{
        insert_uppercase, run_terminal_recipe, terminal_recipe, ProcessingLimits, Processor,
        RustAdapter, TextProcessor,
    };
    use brook::{Limits, ManualClock, Store};
    use std::{collections::BTreeMap, sync::Arc};

    #[test]
    fn config_rejected_by_consumer_factory_before_registration() {
        for config in [
            json!({}),
            json!({"prefix": "ok", "extra": true}),
            json!({"prefix": "x".repeat(33)}),
        ] {
            assert!(prefix_graph(Version::new("stdout", 1), config).is_err());
        }
    }

    #[test]
    fn heterogeneous_registry_and_atomic_commit_through_public_api() -> Result<()> {
        // Deliberately local: this does not implement descriptor registration.
        // Test fixture only: cleanup below covers success. Errors abort the test
        // and drop the temporary store; production hosts must settle every exit.
        let registry: BTreeMap<(String, u32), Box<dyn Processor>> = BTreeMap::from([
            (
                ("prefix".into(), 1),
                Box::new(RustAdapter(Prefix)) as Box<dyn Processor>,
            ),
            (
                ("identity".into(), 1),
                Box::new(RustAdapter(TextProcessor("identity"))) as Box<dyn Processor>,
            ),
        ]);
        assert_eq!(registry.len(), 2);
        let directory = tempfile::tempdir()?;
        let clock = Arc::new(ManualClock::default());
        let mut store = Store::open(directory.path(), Limits::default(), clock.clone())?;
        store.configure_processing(ProcessingLimits::default())?;
        let client = store.terminal_client(TerminalIdentity::local())?;
        let binding = Version::new("stdout", 1);
        store.bind_terminal(&binding, &client)?;
        let graph = prefix_graph(binding, json!({"prefix": "note: "}))?;
        store.validate_graph(&graph)?;
        store.register_graph(&graph)?;
        let first = store.submit_terminal(&client, "terminal", 1, "first", json!("message"))?;
        assert_eq!(
            first,
            store.submit_terminal(&client, "terminal", 1, "first", json!("message"))?
        );
        let lease = store.claim_processing(first, "consumer", 100)?;
        let attempt = store.prepare_processing(&lease, first)?;
        assert!(store
            .execute_processor(&attempt, registry[&("identity".into(), 1)].as_ref())
            .is_err());
        let proposal =
            store.execute_processor(&attempt, registry[&("prefix".into(), 1)].as_ref())?;
        let committed = store.commit_processing(&attempt, &proposal)?;
        assert_eq!(committed.deliveries.len(), 1);
        assert_eq!(
            committed.deliveries,
            store.commit_processing(&attempt, &proposal)?.deliveries
        );
        store.release_processing(&lease)?;
        let terminal = committed.deliveries[0];
        let lease = store.claim_processing(terminal, "consumer", 100)?;
        let effect = store.begin_terminal(&lease, terminal, &client)?;
        let mut output = Vec::new();
        store.print_terminal(&effect, &mut output)?;
        store.release_processing(&lease)?;
        assert_eq!(output, b"note: message\n");
        drop(store);

        let mut store = Store::open(directory.path(), Limits::default(), clock)?;
        let second = store.submit_terminal(&client, "terminal", 1, "second", json!("again"))?;
        let lease = store.claim_processing(second, "consumer", 100)?;
        let attempt = store.prepare_processing(&lease, second)?;
        let proposal =
            store.execute_processor(&attempt, registry[&("prefix".into(), 1)].as_ref())?;
        assert_eq!(proposal.state, json!({"processed": 2}));
        store.commit_processing(&attempt, &proposal)?;
        store.release_processing(&lease)?;

        let identity_graph =
            PipelineBuilder::terminal(&TerminalIdentity::local(), Version::new("stdout", 1))
                .pipeline("identity-example")
                .prepend("identity", Version::new("identity", 1), Value::Null)?
                .build();
        store.register_graph(&identity_graph)?;
        let identity_input = store.submit_terminal(
            &client,
            "identity-example",
            1,
            "identity-call",
            json!("unchanged"),
        )?;
        let lease = store.claim_processing(identity_input, "consumer", 100)?;
        let attempt = store.prepare_processing(&lease, identity_input)?;
        let proposal =
            store.execute_processor(&attempt, registry[&("identity".into(), 1)].as_ref())?;
        assert_eq!(proposal.outputs, [json!("unchanged")]);
        assert_eq!(proposal.state, json!(1));
        let committed = store.commit_processing(&attempt, &proposal)?;
        assert_eq!(committed.deliveries.len(), 1);
        store.release_processing(&lease)?;
        Ok(())
    }

    #[test]
    fn logical_branch_accepts_inserted_transform_without_router_change() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let mut store = Store::open(
            directory.path(),
            Limits::default(),
            Arc::new(ManualClock::default()),
        )?;
        store.configure_processing(ProcessingLimits::default())?;
        let identity = TerminalIdentity::local();
        let client = store.terminal_client(identity.clone())?;
        let binding = Version::new("stdout", 1);
        store.bind_terminal(&binding, &client)?;
        let mut graph = terminal_recipe(&identity, binding)?;
        let router_code = graph.nodes["route"].code.clone();
        let router_config = graph.nodes["route"].config.clone();
        insert_uppercase(&mut graph)?;
        assert_eq!(graph.nodes["route"].code, router_code);
        assert_eq!(graph.nodes["route"].config, router_config);
        store.register_graph(&graph)?;
        store.submit_terminal(&client, "terminal", 1, "route-example", json!("message"))?;
        let mut output = Vec::new();
        assert_eq!(run_terminal_recipe(&mut store, &client, &mut output)?, 4);
        assert_eq!(output, b"MESSAGE\n");
        Ok(())
    }

    #[test]
    fn current_builder_returns_unvalidated_repeated_route() -> Result<()> {
        let graph =
            PipelineBuilder::terminal(&TerminalIdentity::local(), Version::new("stdout", 1))
                .prepend(
                    "router",
                    Version::new("router", 1),
                    json!({"branch": "first"}),
                )?
                .route("router", "first")?
                .route("router", "second")?
                .build();
        assert!(graph.nodes["router"].branches["second"].is_empty());
        assert!(graph.validate(&ProcessingLimits::default()).is_err());
        Ok(())
    }
}
