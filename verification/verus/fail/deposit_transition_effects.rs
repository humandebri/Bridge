#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
use vstd::prelude::*;
verus! {
fn mint_cannot_charge_service_fee_again()
    -> (result: (u128, u128, u128, u128, u128, u128, u128))
    ensures result == (0u128, 0u128, 0u128, 1u128, 10u128, 0u128, 10u128)
{
    kernel::deposit_transition_effects(4, 7, 11, 10, 1, 0)
}
}
fn main() {}
