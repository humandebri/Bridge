use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! { proof fn fee_above_maximum_is_allowed()
    ensures kernel::service_fee_change_allowed_spec(11, 1, 10) {} }
fn main() {}
