use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn reservation_wraps_at_u128_max()
    -> (result: Option<kernel::ReservationDecision>)
    ensures result is Some
{
    kernel::reservation_decision(u128::MAX, 1)
}
}
fn main() {}
