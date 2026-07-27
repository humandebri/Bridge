use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn definitive_failure_is_retained() -> (result: kernel::FundingAttemptDecision)
    ensures result == kernel::FundingAttemptDecision::Retain
{
    kernel::funding_attempt_decision(3)
}
}
fn main() {}
