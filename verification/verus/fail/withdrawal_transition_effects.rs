#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
use vstd::prelude::*;
verus! {
fn successful_release_cannot_credit_the_full_service_fee()
    -> (result: Option<(u8, u128, u128, u128)>)
    ensures result == Some((2u8, 95u128, 10u128, 100u128))
{
    kernel::withdrawal_transition_effects(1, 2, 90, 5, 10)
}
}
fn main() {}
