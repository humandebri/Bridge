#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
use vstd::prelude::*;
verus! {
proof fn mint_cannot_charge_service_fee_again()
    ensures kernel::deposit_numeric_effects_spec(4, 7, 11, 10, 1, 0)
        == (0int, 0int, 0int, 1int, 10int, 0int, 10int)
{}
}
fn main() {}
