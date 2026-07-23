use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn paid_withdrawal_is_indexed() ensures kernel::withdrawal_liability_indexed_spec(2) {} }
fn main() {}
