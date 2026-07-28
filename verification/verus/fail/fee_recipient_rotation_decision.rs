use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn unauthorized_rotation_is_allowed() -> (result: kernel::FeeRecipientRotationDecision)
    ensures result == kernel::FeeRecipientRotationDecision::Allow
{
    kernel::fee_recipient_rotation_decision(false, false, false, 32, 0)
}
}
fn main() {}
