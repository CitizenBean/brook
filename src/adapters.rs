use crate::{Context, HarnessFailure, SendAttempt, SendOutcome};

/// Portable fresh-run boundary. No provider transcript or suspended runtime state.
/// The host must enforce execution time/memory limits before using untrusted harnesses.
pub trait Harness {
    fn name(&self) -> &'static str;
    /// Return at most output_budget UTF-8 bytes or a typed failure. The host must
    /// settle failures through Store::fail_job to release the active session slot.
    fn run(&self, context: &Context, output_budget: usize) -> Result<String, HarnessFailure>;
}
pub struct EchoHarness;
impl Harness for EchoHarness {
    fn name(&self) -> &'static str {
        "echo-v1"
    }
    fn run(&self, c: &Context, output_budget: usize) -> Result<String, HarnessFailure> {
        format_outcome(
            format!("request {}: ", c.request),
            &c.outcome,
            output_budget,
        )
    }
}
pub struct SummaryHarness;
impl Harness for SummaryHarness {
    fn name(&self) -> &'static str {
        "summary-v1"
    }
    fn run(&self, c: &Context, output_budget: usize) -> Result<String, HarnessFailure> {
        format_outcome(
            format!(
                "request {} at revision {}: {} events; ",
                c.request,
                c.revision,
                c.history.len()
            ),
            &c.outcome,
            output_budget,
        )
    }
}
// Decorative prefixes are optional; preserve the complete outcome at the limit.
fn format_outcome(prefix: String, outcome: &str, budget: usize) -> Result<String, HarnessFailure> {
    if outcome.len() > budget {
        return Err(HarnessFailure::OutputBudgetExceeded);
    }
    if prefix.len() <= budget - outcome.len() {
        Ok(prefix + outcome)
    } else {
        Ok(outcome.to_owned())
    }
}
/// No network access or real external effects. The authority owns send admission.
pub struct FakeSink {
    pub outcome: SendOutcome,
}
impl FakeSink {
    pub fn send(&self, _attempt: &SendAttempt) -> SendOutcome {
        self.outcome
    }
}
