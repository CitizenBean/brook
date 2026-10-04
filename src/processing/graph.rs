use super::types::*;
use crate::{Error, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

impl Graph {
    pub fn validate(&self, limits: &ProcessingLimits) -> Result<()> {
        name(&self.namespace)?;
        name(&self.pipeline)?;
        name(&self.entry)?;
        if self.version == 0
            || self.nodes.is_empty()
            || self.nodes.len() > 64
            || !self.nodes.contains_key(&self.entry)
            || serde_json::to_vec(self)?.len() > 262144
        {
            return Err(Error::Invalid("invalid graph size, version or entry"));
        }
        for (id, n) in &self.nodes {
            name(id)?;
            n.code.validate()?;
            n.input.identity.validate()?;
            n.output.identity.validate()?;
            n.state_schema.identity.validate()?;
            if !n.state_schema.accepts(&n.initial_state)
                || serde_json::to_vec(&n.initial_state)?.len() > limits.state_bytes
            {
                return Err(Error::Invalid("invalid initial state"));
            }
            if n.config_version == 0
                || serde_json::to_vec(&n.config)?.len() > limits.input_bytes
                || n.branches.len() > limits.fanout
            {
                return Err(Error::Capacity);
            }
            match &n.kind {
                NodeKind::Processor { routing: false } if !n.branches.is_empty() => {
                    return Err(Error::Invalid("ordinary processor cannot declare branches"))
                }
                NodeKind::Terminal { binding } => {
                    binding.validate()?;
                    if !n.next.is_empty()
                        || !n.branches.is_empty()
                        || n.input != Schema::text()
                        || n.code != Version::new("terminal", 1)
                    {
                        return Err(Error::Invalid("terminal must be a text leaf"));
                    }
                }
                _ => {}
            }
            let targets: Vec<_> = n.next.iter().chain(n.branches.values().flatten()).collect();
            if targets.len() > limits.fanout {
                return Err(Error::Capacity);
            }
            for (branch, destinations) in &n.branches {
                name(branch)?;
                if destinations.is_empty() {
                    return Err(Error::Invalid("empty branch"));
                }
            }
            for paths in std::iter::once(&n.next).chain(n.branches.values()) {
                if paths.iter().collect::<BTreeSet<_>>().len() != paths.len() {
                    return Err(Error::Invalid("duplicate path"));
                }
            }
            for target in targets {
                let dest = self
                    .nodes
                    .get(target)
                    .ok_or(Error::Invalid("unknown graph destination"))?;
                if n.output != dest.input {
                    return Err(Error::Invalid("edge schemas differ"));
                }
            }
        }
        fn visit<'a>(
            g: &'a Graph,
            id: &'a str,
            visiting: &mut BTreeSet<&'a str>,
            done: &mut BTreeSet<&'a str>,
        ) -> Result<()> {
            if done.contains(id) {
                return Ok(());
            }
            if !visiting.insert(id) {
                return Err(Error::Invalid("graph cycles are unsupported"));
            }
            let n = &g.nodes[id];
            for target in n.next.iter().chain(n.branches.values().flatten()) {
                visit(g, target, visiting, done)?;
            }
            visiting.remove(id);
            done.insert(id);
            Ok(())
        }
        let mut done = BTreeSet::new();
        for id in self.nodes.keys() {
            visit(self, id, &mut BTreeSet::new(), &mut done)?;
        }
        Ok(())
    }
}
/// A small text-pipeline builder; advanced callers can use Graph directly.
pub struct PipelineBuilder {
    graph: Graph,
    tail: String,
}
impl PipelineBuilder {
    pub fn terminal(identity: &TerminalIdentity, binding: Version) -> Self {
        let node = Node {
            code: Version::new("terminal", 1),
            config_version: 1,
            config: Value::Null,
            input: Schema::text(),
            output: Schema::text(),
            state_schema: Schema {
                identity: Version::new("counter", 1),
                shape: JsonShape::NullOrInteger,
            },
            initial_state: Value::Null,
            kind: NodeKind::Terminal { binding },
            next: vec![],
            branches: BTreeMap::new(),
        };
        Self {
            graph: Graph {
                namespace: identity.namespace.clone(),
                pipeline: "terminal".into(),
                version: 1,
                entry: "output".into(),
                nodes: BTreeMap::from([("output".into(), node)]),
            },
            tail: "output".into(),
        }
    }
    pub fn version(mut self, version: u32) -> Self {
        self.graph.version = version;
        self
    }
    pub fn pipeline(mut self, name: impl Into<String>) -> Self {
        self.graph.pipeline = name.into();
        self
    }
    /// Prepend a transform to the configured path. Router implementations never name this node.
    pub fn prepend(mut self, id: impl Into<String>, code: Version, config: Value) -> Result<Self> {
        let id = id.into();
        if self.graph.nodes.contains_key(&id) {
            return Err(Error::Conflict);
        }
        let n = Node {
            code,
            config_version: 1,
            config,
            input: Schema::text(),
            output: Schema::text(),
            state_schema: Schema {
                identity: Version::new("counter", 1),
                shape: JsonShape::NullOrInteger,
            },
            initial_state: Value::Null,
            kind: NodeKind::Processor { routing: false },
            next: vec![self.tail.clone()],
            branches: BTreeMap::new(),
        };
        self.graph.nodes.insert(id.clone(), n);
        self.graph.entry = id.clone();
        self.tail = id;
        Ok(self)
    }
    /// Convert a configured node to a router with one logical branch to its existing path.
    pub fn route(mut self, id: &str, branch: &str) -> Result<Self> {
        let n = self
            .graph
            .nodes
            .get_mut(id)
            .ok_or(Error::Invalid("unknown node"))?;
        n.kind = NodeKind::Processor { routing: true };
        n.branches
            .insert(branch.into(), std::mem::take(&mut n.next));
        Ok(self)
    }
    pub fn build(self) -> Graph {
        self.graph
    }
}
