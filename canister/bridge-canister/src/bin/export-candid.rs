fn main() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("bridge.did");
    std::fs::write(path, bridge_canister::generated_candid_interface())
        .expect("write generated bridge.did");
}
