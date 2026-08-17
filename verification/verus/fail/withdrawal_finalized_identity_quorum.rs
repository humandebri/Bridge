use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;

verus! {
proof fn one_provider_is_enough()
    ensures kernel::withdrawal_finalized_identity_quorum_spec(
        Some((102int, 7int)), None, None) == Some((102int, 7int))
{}
}
fn main() {}
