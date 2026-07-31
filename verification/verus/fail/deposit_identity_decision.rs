use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn processed_deposit_is_allowed() -> (result: kernel::DepositIdentityDecision)
    ensures result == kernel::DepositIdentityDecision::Allow
{
    kernel::deposit_identity_decision(true)
}
}
fn main() {}
