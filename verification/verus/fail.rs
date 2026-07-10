// verification/verus: provide a deliberate postcondition violation proving Verus is active.
use vstd::prelude::*;

verus! {

fn invalid_increment(value: u64) -> (result: u64)
    requires
        value < u64::MAX,
    ensures
        result == value + 1,
{
    value
}

fn main() {}

} // verus!
