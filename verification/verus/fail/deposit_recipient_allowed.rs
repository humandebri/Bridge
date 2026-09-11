use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn bridge_can_receive_deposit()
    ensures kernel::deposit_recipient_allowed_spec(false, true, false) {} }
fn main() {}
