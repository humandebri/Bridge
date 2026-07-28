#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
use vstd::prelude::*;
verus! {
proof fn signature_can_be_installed_without_dispatch()
    ensures kernel::signature_install_allowed_spec(false, true, true)
{}
}
fn main() {}
