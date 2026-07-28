use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn confirmed_operation_is_indexed() ensures kernel::evm_operation_indexed_spec(3) {} }
fn main() {}
