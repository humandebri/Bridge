use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn conflicting_withdrawal_block_must_not_be_accepted()
    ensures kernel::withdrawal_ledger_block_transition_spec(Some(7), 1, 8).is_some()
{} }
fn main() {}
