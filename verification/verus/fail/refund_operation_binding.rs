use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn refund_without_calldata_binding_is_valid()
    ensures kernel::refund_operation_binding_spec(true, true, true, false) {} }
fn main() {}
