use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;

verus! {
proof fn inconsistent_index_silently_has_no_liabilities()
    ensures kernel::nonterminal_deposit_reservation_count_spec(1int, 2int) == Some(0int)
{}
}

fn main() {}
