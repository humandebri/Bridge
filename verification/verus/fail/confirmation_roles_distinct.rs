use vstd::prelude::*;

#[allow(unused_macros)]
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;

verus! {

fn overlapping_pause_and_relayer_roles_are_distinct() -> (result: bool)
    ensures result,
{
    kernel::confirmation_roles_distinct(false, true, false, true)
}

}

fn main() {}
