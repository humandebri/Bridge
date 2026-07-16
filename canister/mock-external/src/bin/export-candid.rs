fn main() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("mock.did");
    std::fs::write(path, mock_external::generated_candid_interface())
        .expect("write generated mock.did");
}
