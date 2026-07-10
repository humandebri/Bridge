//! IC boundary for the SNS–Base Bridge.
//!
//! Phase 0 exports an empty Candid service on purpose. Asset-moving and administrative methods
//! remain absent until their contracts and authorization rules are fixed in later phases.

ic_cdk::export_candid!();
