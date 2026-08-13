use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
proof fn terminal_refund_is_indexed()
    ensures kernel::deposit_nonterminal_indexed_spec(8)
{}
}
fn main() {}
