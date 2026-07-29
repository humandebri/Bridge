use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn early_complete_absence_releases() -> (result: kernel::FundingReconciliationDecision)
    ensures result == kernel::FundingReconciliationDecision::Release
{
    kernel::funding_reconciliation_decision(true, false, true)
}
}
fn main() {}
