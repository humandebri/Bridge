use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn noncontroller_seals_bootstrap() ensures kernel::operational_config_seal_caller_authorized_spec(false, true) {} }
fn main() {}
