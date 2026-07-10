// verification/verus: prove a minimal executable Rust function before domain proofs exist.
use vstd::prelude::*;

verus! {

fn bounded_increment(value: u64) -> (result: u64)
    requires
        value < u64::MAX,
    ensures
        result == value + 1,
{
    value + 1
}

fn main() {
    let result = bounded_increment(0);
    assert(result == 1);
}

} // verus!
