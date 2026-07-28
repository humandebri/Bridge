use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn processed_deposit_is_refundable()
    ensures kernel::expiry_refund_allowed_spec(true, true, 101, 100) {} }
fn main() {}
