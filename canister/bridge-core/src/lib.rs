//! Pure Bridge domain core.
//!
//! Phase 0 intentionally contains no business logic. Deposit and Withdrawal state machines
//! enter this dependency-free crate in Phase 2 so Verus can verify them without IC runtime I/O.
