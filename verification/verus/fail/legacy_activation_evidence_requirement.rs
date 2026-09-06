use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! {
proof fn pending_activation_is_misclassified_as_execute()
    ensures kernel::legacy_activation_evidence_requirement_spec(true, true, true, false, true) == 2
{}
}
fn main() {}
