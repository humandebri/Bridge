#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
use vstd::prelude::*;
verus! {
fn replay_cannot_be_rejected() -> (result: kernel::DepositTransitionDecision)
    ensures match result {
        kernel::DepositTransitionDecision::Reject => true,
        _ => false,
    }
{
    kernel::deposit_transition_decision(kernel::DepositTransitionInput {
        state: 0,
        event: 0,
        guard: kernel::DepositEventGuard::Funding,
        same_payload: true,
        gross_amount: 11,
        net_amount: 10,
        service_fee: 1,
        reserved_amount: 0,
    })
}
}
fn main() {}
