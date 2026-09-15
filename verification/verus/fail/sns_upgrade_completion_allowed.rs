use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn personal_upgrade_proves_sns_execution() ensures kernel::sns_upgrade_completion_allowed_spec(false, 5, 3, 2, 6) {} }
fn main() {}
