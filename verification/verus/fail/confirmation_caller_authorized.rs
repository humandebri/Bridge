use vstd::prelude::*;

#[allow(unused_macros)]
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;

verus! {

fn anonymous_relayer_is_authorized() -> (result: bool)
    ensures result,
{
    kernel::confirmation_caller_authorized(false, true, false, false)
}

}

fn main() {}
