use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn fee_above_service_is_allowed()
    ensures kernel::ledger_fee_reprice_allowed_spec(10, 11, true, false) {} }
fn main() {}
