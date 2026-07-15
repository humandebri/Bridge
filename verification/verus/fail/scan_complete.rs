use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn incomplete_scan_is_complete() ensures kernel::scan_complete_spec(1, 1, 1, true, false) {} }
fn main() {}
