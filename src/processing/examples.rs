use super::*;
use crate::{Error, Result, Store};
use serde_json::{json, Value};
/// Deliberately small native examples; code identity is checked by the host.
pub struct TextProcessor(pub &'static str);
impl TypedProcessor for TextProcessor {
    type Input = String;
    type State = Option<u64>;
    type Output = String;
    fn code(&self) -> Version {
        Version::new(self.0, 1)
    }
    fn process(
        &self,
        input: String,
        state: Option<u64>,
        config: &Value,
    ) -> Result<TypedProposal<Self::State, String>> {
        let count = state.unwrap_or(0).checked_add(1).ok_or(Error::Capacity)?;
        let (outputs, routing, reason) = match self.0 {
            "uppercase" => (
                vec![input.to_uppercase()],
                Routing::ConfiguredNext,
                "uppercase transform",
            ),
            "filter" if input.trim().is_empty() => (
                vec![],
                Routing::Suppress("blank input".into()),
                "blank input",
            ),
            "filter" | "identity" => (vec![input], Routing::ConfiguredNext, "configured path"),
            "router" => {
                let branch = config
                    .get("branch")
                    .and_then(Value::as_str)
                    .ok_or(Error::Invalid("router branch config"))?;
                (
                    vec![input],
                    Routing::SelectedBranches(vec![branch.into()]),
                    "configured logical branch",
                )
            }
            _ => return Err(Error::Invalid("unknown example processor")),
        };
        Ok(TypedProposal {
            reason: reason.into(),
            state: Some(count),
            outputs,
            routing,
        })
    }
}
/// Zero-config local recipe: input -> blank filter -> logical router -> terminal.
pub fn terminal_recipe(identity: &TerminalIdentity, binding: Version) -> Result<Graph> {
    Ok(PipelineBuilder::terminal(identity, binding)
        .prepend(
            "route",
            Version::new("router", 1),
            json!({"branch":"display"}),
        )?
        .route("route", "display")?
        .prepend("filter", Version::new("filter", 1), Value::Null)?
        .build())
}
/// A graph-only override: router code/config/branch name stay unchanged.
pub fn insert_uppercase(graph: &mut Graph) -> Result<()> {
    let mut transform = graph
        .nodes
        .get("filter")
        .ok_or(Error::Invalid("recipe filter missing"))?
        .clone();
    transform.code = Version::new("uppercase", 1);
    transform.next = vec!["output".into()];
    if graph.nodes.contains_key("uppercase") {
        return Err(Error::Conflict);
    }
    graph
        .nodes
        .get_mut("route")
        .ok_or(Error::Invalid("recipe router missing"))?
        .branches
        .insert("display".into(), vec!["uppercase".into()]);
    graph.nodes.insert("uppercase".into(), transform);
    Ok(())
}
/// Bounded manual runner, not a background scheduler or agent loop.
pub fn run_terminal_recipe(
    store: &mut Store,
    client: &TerminalClient,
    writer: &mut impl std::io::Write,
) -> Result<usize> {
    let mut completed = 0;
    for _ in 0..64 {
        let Some(id) = store
            .pending_terminal(client, "terminal", 1)?
            .first()
            .copied()
        else {
            break;
        };
        let status = store.inspect_processing(id)?;
        let graph = store.effective_graph(
            &status.scope.namespace,
            &status.scope.pipeline,
            status.graph_version,
        )?;
        let node = &graph.nodes[&status.scope.node];
        let lease = store.claim_processing(id, "manual-runner", 60000)?;
        let result = (|| {
            if matches!(node.kind, NodeKind::Terminal { .. }) {
                store
                    .begin_terminal(&lease, id, client)
                    .and_then(|a| store.print_terminal(&a, writer))
            } else {
                let a = store.prepare_processing(&lease, id)?;
                let code = match node.code.id.as_str() {
                    "identity" => "identity",
                    "filter" => "filter",
                    "uppercase" => "uppercase",
                    "router" => "router",
                    _ => "unsupported",
                };
                match store
                    .execute_processor(&a, &RustAdapter(TextProcessor(code)))
                    .and_then(|p| store.commit_processing(&a, &p))
                {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        store.fail_processing(
                            &a,
                            "manual processor failed validation or execution",
                            false,
                        )?;
                        Err(e)
                    }
                }
            }
        })();
        // Run cleanup even when prepare/execution/failure recording returns early.
        let released = store.release_processing(&lease);
        result?;
        released?;
        completed += 1;
    }
    Ok(completed)
}
