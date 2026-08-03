use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! {
proof fn missing_runtime_binding_is_accepted()
    ensures kernel::runtime_attestation_matches_spec(true, true, false)
{}
}
fn main() {}
