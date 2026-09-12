use bridge_core::{
    withdrawal_common_checkpoint, withdrawal_finalized_checkpoint_quorum,
    withdrawal_finalized_identity_quorum, withdrawal_id_is_admissible, WithdrawalFinalizedIdentity,
};

#[test]
fn staggered_finalized_heads_select_the_conservative_common_checkpoint() {
    let at = |height, byte| WithdrawalFinalizedIdentity {
        block_number: height,
        block_hash: [byte; 32],
    };
    assert_eq!(
        withdrawal_common_checkpoint(Some(at(100, 1)), Some(at(101, 2)), Some(at(102, 3))),
        Some(101)
    );
    assert_eq!(
        withdrawal_common_checkpoint(Some(at(100, 1)), None, Some(at(102, 3))),
        Some(100)
    );
    assert_eq!(
        withdrawal_common_checkpoint(Some(at(99, 1)), Some(at(101, 2)), Some(at(102, 3))),
        Some(101)
    );
    assert_eq!(
        withdrawal_common_checkpoint(Some(at(100, 1)), Some(at(101, 2)), Some(at(999, 3))),
        Some(101)
    );
    assert_eq!(
        withdrawal_common_checkpoint(Some(at(100, 1)), None, None),
        None
    );
}

#[test]
fn withdrawal_checkpoint_quorum_excludes_votes_from_heads_below_the_checkpoint() {
    let at = |height, byte| WithdrawalFinalizedIdentity {
        block_number: height,
        block_hash: [byte; 32],
    };
    let finalized_heads = [Some(at(90, 1)), Some(at(100, 2)), Some(at(110, 3))];

    assert_eq!(
        withdrawal_finalized_checkpoint_quorum(
            finalized_heads,
            [
                Some(at(100, 0xaa)),
                Some(at(100, 0xaa)),
                Some(at(100, 0xbb))
            ],
            100,
        ),
        None
    );
    assert_eq!(
        withdrawal_finalized_checkpoint_quorum(
            [Some(at(100, 1)), Some(at(101, 2)), Some(at(102, 3))],
            [
                Some(at(101, 0xaa)),
                Some(at(101, 0xbb)),
                Some(at(101, 0xbb))
            ],
            101,
        ),
        Some(at(101, 0xbb))
    );
}

#[test]
fn withdrawal_admission_boundary_uses_the_full_big_endian_uint256() {
    let mut minimum = [0u8; 32];
    minimum[15] = 1;
    let mut below = minimum;
    below[15] = 0;
    below[31] = u8::MAX;
    let mut above = minimum;
    above[31] = 1;

    assert!(!withdrawal_id_is_admissible(&below, &minimum));
    assert!(withdrawal_id_is_admissible(&minimum, &minimum));
    assert!(withdrawal_id_is_admissible(&above, &minimum));
    assert!(!withdrawal_id_is_admissible(&minimum, &[0; 32]));
    assert!(!withdrawal_id_is_admissible(&minimum, &[1; 31]));
}

#[test]
fn withdrawal_finality_quorum_requires_an_exact_two_provider_checkpoint() {
    let first = WithdrawalFinalizedIdentity {
        block_number: 100,
        block_hash: [0xaa; 32],
    };
    let third = WithdrawalFinalizedIdentity {
        block_number: 102,
        block_hash: [0xbb; 32],
    };
    assert_eq!(
        withdrawal_finalized_identity_quorum(Some(first), Some(first), Some(third)),
        Some(first)
    );
    assert_eq!(
        withdrawal_finalized_identity_quorum(Some(third), Some(first), Some(third)),
        Some(third)
    );
    assert_eq!(
        withdrawal_finalized_identity_quorum(Some(first), None, Some(third)),
        None
    );
    assert_eq!(
        withdrawal_finalized_identity_quorum(
            Some(first),
            Some(WithdrawalFinalizedIdentity {
                block_number: 100,
                block_hash: [0xcc; 32],
            }),
            Some(third),
        ),
        None
    );
    assert_eq!(
        withdrawal_finalized_identity_quorum(Some(third), None, None),
        None
    );
}
