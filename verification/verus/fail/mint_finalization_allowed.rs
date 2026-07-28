use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn reverted_receipt_is_minted()
    ensures kernel::mint_finalization_allowed_spec(true, false, 7, 8) {} }
fn main() {}
