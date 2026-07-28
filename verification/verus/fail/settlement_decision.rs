use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn settlement_accepts_fee_inversion() -> (result: Option<kernel::SettlementDecision>)
    ensures result matches Some(decision) && decision.reserve_credit == 0
{
    kernel::settlement_decision(10, 2, 1)
}
}
fn main() {}
