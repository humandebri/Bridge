#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
use vstd::prelude::*;
verus! {
fn successful_hold_cannot_credit_the_full_service_fee()
    -> (result: Option<(u8, u128, u128, u128)>)
    ensures result == Some((2u8, 95u128, 10u128, 100u128))
{
    kernel::withdrawal_transition_effects(3, 4, 90, 5, 10)
}

fn absent_hold_cannot_credit_a_reserve_effect()
    -> (result: Option<(u8, u128, u128, u128)>)
    ensures result == Some((1u8, 0u128, 5u128, 0u128))
{
    kernel::withdrawal_transition_effects(3, 5, 90, 5, 10)
}
}
fn main() {}
