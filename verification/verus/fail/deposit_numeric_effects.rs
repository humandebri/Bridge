#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
use vstd::prelude::*;
verus! {
proof fn mint_omits_pending_liability_debit()
    ensures kernel::deposit_numeric_effects_spec(3, 7, 11, 10, 1, 10)
        == (0int, 0int, 10int, 1int, 0int, 0int, 10int)
{}
}
fn main() {}
