use bridge_core::{
    administrator_authorized, audit_next, can_assign_nonce, candidate_precedes,
    checked_requirement, counter_delta, evidence_matches, mint_admission_total, next_attempt,
    nonce_next, payout_allowed, payout_debit, refund_allowed, replay_matches, resources_sufficient,
    scan_complete, scheduler_priority, terminal_retry_fee,
};

#[test]
fn boolean_decisions_are_exhaustive() {
    for pending in [false, true] {
        for attempted in [false, true] {
            assert_eq!(refund_allowed(pending, attempted), pending && !attempted);
        }
    }
    for old in [false, true] {
        for new in [false, true] {
            let expected = match (old, new) {
                (false, true) => 1,
                (true, false) => -1,
                _ => 0,
            };
            assert_eq!(counter_delta(old, new), expected);
        }
    }
    for request in [false, true] {
        for hold in [false, true] {
            for transfer in [false, true] {
                for open in [false, true] {
                    for evidence in [false, true] {
                        assert_eq!(
                            evidence_matches(request, hold, transfer, open, evidence),
                            request && hold && transfer && open && evidence
                        );
                    }
                }
            }
        }
    }
    assert!(replay_matches(true));
    assert!(!replay_matches(false));
}

#[test]
fn reserve_boundaries_and_overflow_are_checked() {
    assert_eq!(checked_requirement(0, 0, 0), Some(0));
    assert_eq!(checked_requirement(7, 3, 4), Some(19));
    assert_eq!(
        checked_requirement(u128::MAX, 0, u128::MAX),
        Some(u128::MAX)
    );
    assert_eq!(checked_requirement(u128::MAX, 1, 1), None);
    assert_eq!(checked_requirement(0, u128::MAX, 2), None);
    for eth_ok in [false, true] {
        for cycles_ok in [false, true] {
            let eth = if eth_ok { 10 } else { 9 };
            let cycles = if cycles_ok { 20 } else { 19 };
            assert_eq!(
                resources_sufficient(eth, 10, cycles, 20),
                eth_ok && cycles_ok
            );
        }
    }
}

#[test]
fn mint_admission_includes_existing_reservations_and_checks_overflow() {
    assert_eq!(mint_admission_total(90, 9, 1), Some(100));
    assert_eq!(mint_admission_total(90, 10, 1), Some(101));
    assert_eq!(mint_admission_total(u128::MAX, 0, 0), Some(u128::MAX));
    assert_eq!(mint_admission_total(u128::MAX, 1, 0), None);
    assert_eq!(mint_admission_total(0, u128::MAX, 1), None);
}

#[test]
fn scheduler_nonce_and_audit_boundaries_are_deterministic() {
    assert_eq!(scheduler_priority(0), 0);
    assert_eq!(scheduler_priority(1), 0);
    assert_eq!(scheduler_priority(2), 1);
    assert!(candidate_precedes(0, 99, 1, 0));
    assert!(candidate_precedes(0, 1, 0, 2));
    assert!(!candidate_precedes(0, 2, 0, 1));
    assert!(!can_assign_nonce(false, false));
    assert!(!can_assign_nonce(true, true));
    assert!(can_assign_nonce(true, false));
    assert_eq!(nonce_next(0), Some(1));
    assert_eq!(nonce_next(u64::MAX), None);
    assert_eq!(audit_next(0), Some(1));
    assert_eq!(audit_next(u64::MAX), None);
}

#[test]
fn payout_and_authorization_tables_are_exhaustive() {
    assert!(payout_allowed(12, 2, 7, 3));
    assert!(!payout_allowed(11, 2, 7, 3));
    assert!(!payout_allowed(u128::MAX, 0, u128::MAX, 1));
    assert_eq!(payout_debit(true, 7, 3), Some(10));
    assert_eq!(payout_debit(false, 7, 3), Some(0));
    assert_eq!(payout_debit(true, u128::MAX, 1), None);
    for action in 0..=4 {
        for pause in [false, true] {
            for finance in [false, true] {
                for governance in [false, true] {
                    let expected = (action == 0 && pause)
                        || ((action == 2 || action == 3) && finance)
                        || ((action == 1 || action == 4) && governance);
                    assert_eq!(
                        administrator_authorized(action, pause, finance, governance),
                        expected
                    );
                }
            }
        }
    }
}

#[test]
fn finite_scan_domain_finds_inclusive_tip_counterexample() {
    for next in 0..=3 {
        for tip in 0..=3 {
            for watermark in 0..=3 {
                for archives in [false, true] {
                    for matched in [false, true] {
                        let accepted = scan_complete(next, tip, watermark, archives, matched);
                        if accepted {
                            assert!(next > tip);
                            assert!(watermark >= tip);
                            assert!(archives);
                            assert!(!matched);
                        }
                    }
                }
            }
        }
    }
    assert!(!scan_complete(3, 3, 3, true, false));
    assert!(scan_complete(4, 3, 3, true, false));
}

#[test]
fn attempt_and_fee_boundaries_are_checked() {
    assert_eq!(next_attempt(0), Some(1));
    assert_eq!(next_attempt(u64::MAX - 1), Some(u64::MAX));
    assert_eq!(next_attempt(u64::MAX), None);
    assert_eq!(terminal_retry_fee(true, u128::MAX), u128::MAX);
    assert_eq!(terminal_retry_fee(false, u128::MAX), 0);
}
