use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! { proof fn sealed_config_can_be_resealed()
    ensures kernel::operational_config_seal_allowed_spec(true, true) {} }
fn main() {}
