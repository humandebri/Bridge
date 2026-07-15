use bridge_core::{
    administrator_authorized, audit_next, bad_fee_reprice_amount, can_assign_nonce,
    checked_counter_transition, checked_requirement, counter_delta, deposit_phase_allows,
    deposit_phase_step, evidence_matches, fee_delta_once, mint_admission_total, next_attempt,
    nonce_next, nonce_too_low_is_submitted, payout_allowed, payout_debit, refund_allowed,
    release_transfer_matches, replay_matches, resources_sufficient, scan_complete,
    withdrawal_phase_allows, withdrawal_phase_step,
};

#[test]
fn boolean_decisions_are_exhaustive() {
    for pending in [false, true] {
        for proven_absent in [false, true] {
            assert_eq!(
                refund_allowed(pending, proven_absent),
                pending && proven_absent
            );
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
fn nonce_and_audit_boundaries_are_deterministic() {
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
    assert_eq!(fee_delta_once(false, true, 9), 9);
    assert_eq!(fee_delta_once(true, true, 9), 0);
    assert_eq!(fee_delta_once(false, false, 9), 0);
    assert!(release_transfer_matches(85, 5, 85, 5));
    assert!(!release_transfer_matches(84, 5, 85, 5));
    assert!(!release_transfer_matches(85, 6, 85, 5));
    assert_eq!(checked_counter_transition(7, false, true), Some(8));
    assert_eq!(checked_counter_transition(7, true, false), Some(6));
    assert_eq!(checked_counter_transition(0, true, false), None);
}

#[test]
fn compact_phase_kernels_match_the_legal_transition_graphs() {
    let deposit_edges = [
        (0, 0, 1),
        (0, 1, 5),
        (0, 2, 6),
        (1, 3, 2),
        (2, 4, 3),
        (2, 5, 4),
    ];
    for state in 0..=6 {
        for event in 0..=5 {
            let expected = deposit_edges
                .iter()
                .find(|(from, input, _)| *from == state && *input == event)
                .map_or(state, |(_, _, next)| *next);
            assert_eq!(deposit_phase_step(state, event), expected);
            assert_eq!(deposit_phase_allows(state, event), expected != state);
        }
    }

    let withdrawal_edges = [
        (0, 0, 1),
        (0, 4, 10),
        (1, 1, 1),
        (1, 2, 2),
        (1, 3, 9),
        (1, 4, 10),
        (10, 5, 11),
        (2, 6, 3),
        (3, 7, 5),
        (3, 8, 4),
        (0, 9, 6),
        (11, 9, 6),
        (6, 10, 8),
        (6, 11, 7),
    ];
    for state in 0..=11 {
        for event in 0..=11 {
            let expected = withdrawal_edges
                .iter()
                .find(|(from, input, _)| *from == state && *input == event)
                .map_or(state, |(_, _, next)| *next);
            assert_eq!(
                withdrawal_phase_step(state, event),
                expected,
                "withdrawal state {state}, event {event}"
            );
            assert_eq!(
                withdrawal_phase_allows(state, event),
                expected != state || (state == 1 && event == 1)
            );
        }
    }
}

#[test]
fn bad_fee_and_nonce_conflict_fail_closed() {
    assert_eq!(
        bad_fee_reprice_amount(100, 10, 6, 80, true, false),
        Some(84)
    );
    assert_eq!(bad_fee_reprice_amount(100, 10, 11, 80, true, false), None);
    assert_eq!(bad_fee_reprice_amount(100, 10, 6, 80, false, false), None);
    assert_eq!(bad_fee_reprice_amount(100, 10, 6, 80, true, true), None);
    assert!(nonce_too_low_is_submitted(true, true));
    assert!(!nonce_too_low_is_submitted(true, false));
    assert!(!nonce_too_low_is_submitted(false, true));
}
