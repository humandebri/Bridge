use bridge_core::runtime_attestation_matches;

#[test]
fn runtime_attestation_requires_every_config_binding() {
    assert!(runtime_attestation_matches(true, true, true));
    assert!(!runtime_attestation_matches(false, true, true));
    assert!(!runtime_attestation_matches(true, false, true));
    assert!(!runtime_attestation_matches(true, true, false));
}
