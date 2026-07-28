use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn repeated_payout_debits_again() -> (result: Option<kernel::PayoutDecision>)
    ensures match result {
        Some(decision) => decision.debit == 10,
        None => false,
    }
{
    kernel::payout_decision(100, 0, 7, 3, false)
}
}
fn main() {}
