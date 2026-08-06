use vstd::prelude::*;

#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;

verus! {
proof fn fee_cannot_be_charged_on_mint_finalization()
    ensures kernel::deposit_charge_service_fee_spec(4, 7)
{}
}

fn main() {}
