use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn conflicting_deposit_block_must_not_be_accepted()
    ensures kernel::deposit_ledger_block_transition_spec(Some(7), None, 1, 8).is_some()
{} }
fn main() {}
