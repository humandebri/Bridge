use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn sealed_state_applies_migration() ensures kernel::bootstrap_pause_principal_migration_code_spec(true, true, true, false, true, false, true) == 0 {} }
fn main() {}
