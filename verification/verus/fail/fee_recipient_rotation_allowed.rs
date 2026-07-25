use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! { proof fn pending_payout_allows_rotation()
    ensures kernel::fee_recipient_rotation_allowed_spec(1) {} }
fn main() {}
