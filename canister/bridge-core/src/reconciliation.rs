use crate::{
    Amount, ApplyOutcome, ApplyResult, CoreError, DepositId, DepositRecord, DepositState, HoldId,
    LedgerTransferIdentity, WithdrawalId, WithdrawalRecord, WithdrawalState,
};

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestReference {
    Deposit(DepositId),
    Withdrawal(WithdrawalId),
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldResolution {
    Succeeded { ledger_block_index: u128 },
    Absent { history_watermark: Option<u128> },
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconciliationHoldState {
    Open,
    ResolvedSucceeded { ledger_block_index: u128 },
    ResolvedAbsent { history_watermark: u128 },
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconciliationHoldRecord {
    pub id: HoldId,
    pub request: RequestReference,
    pub transfer: LedgerTransferIdentity,
    pub state: ReconciliationHoldState,
}

impl ReconciliationHoldRecord {
    pub const fn open(
        id: HoldId,
        request: RequestReference,
        transfer: LedgerTransferIdentity,
    ) -> Self {
        Self {
            id,
            request,
            transfer,
            state: ReconciliationHoldState::Open,
        }
    }

    fn resolve(&mut self, resolution: HoldResolution) -> Result<ApplyOutcome, CoreError> {
        use HoldResolution as Resolution;
        use ReconciliationHoldState as State;
        let next = match (self.state, resolution) {
            (State::Open, Resolution::Succeeded { ledger_block_index }) => {
                State::ResolvedSucceeded { ledger_block_index }
            }
            (
                State::Open,
                Resolution::Absent {
                    history_watermark: Some(history_watermark),
                },
            ) => State::ResolvedAbsent { history_watermark },
            (
                State::ResolvedSucceeded {
                    ledger_block_index: current,
                },
                Resolution::Succeeded { ledger_block_index },
            ) if current == ledger_block_index => return Ok(ApplyOutcome::Idempotent),
            (
                State::ResolvedAbsent {
                    history_watermark: current,
                },
                Resolution::Absent {
                    history_watermark: Some(history_watermark),
                },
            ) if current == history_watermark => return Ok(ApplyOutcome::Idempotent),
            (
                _,
                Resolution::Absent {
                    history_watermark: None,
                },
            ) => {
                return Err(CoreError::MissingReconciliationEvidence);
            }
            (State::ResolvedSucceeded { .. } | State::ResolvedAbsent { .. }, _) => {
                return Err(CoreError::ConflictingReplay);
            }
        };
        self.state = next;
        Ok(ApplyOutcome::Applied)
    }
}

pub fn resolve_deposit_hold(
    deposit: &mut DepositRecord,
    hold: &mut ReconciliationHoldRecord,
    resolution: HoldResolution,
) -> Result<ApplyResult, CoreError> {
    if hold.request != RequestReference::Deposit(deposit.id) || hold.transfer != deposit.transfer {
        return Err(CoreError::HoldMismatch);
    }

    let mut next_hold = hold.clone();
    let hold_outcome = next_hold.resolve(resolution)?;
    let mut next_deposit = deposit.clone();
    let request_outcome = match (resolution, &deposit.state) {
        (
            HoldResolution::Succeeded { ledger_block_index },
            DepositState::ReconciliationHold { hold_id },
        ) if *hold_id == hold.id => {
            next_deposit.state = DepositState::Escrowed { ledger_block_index };
            ApplyOutcome::Applied
        }
        (
            HoldResolution::Succeeded { ledger_block_index },
            DepositState::Escrowed {
                ledger_block_index: current,
            }
            | DepositState::MintPending {
                ledger_block_index: current,
                ..
            }
            | DepositState::Minted {
                ledger_block_index: current,
                ..
            },
        ) if *current == ledger_block_index => ApplyOutcome::Idempotent,
        (
            HoldResolution::Absent {
                history_watermark: Some(_),
            },
            DepositState::ReconciliationHold { hold_id },
        ) if *hold_id == hold.id => {
            next_deposit.state = DepositState::PullPending;
            ApplyOutcome::Applied
        }
        (
            HoldResolution::Absent {
                history_watermark: Some(_),
            },
            DepositState::PullPending,
        ) => ApplyOutcome::Idempotent,
        (
            HoldResolution::Absent {
                history_watermark: None,
            },
            _,
        ) => {
            return Err(CoreError::MissingReconciliationEvidence);
        }
        _ => return Err(CoreError::HoldMismatch),
    };
    if hold_outcome != request_outcome {
        return Err(CoreError::ConflictingReplay);
    }

    *deposit = next_deposit;
    *hold = next_hold;
    Ok(match request_outcome {
        ApplyOutcome::Applied => ApplyResult::applied(Amount::ZERO),
        ApplyOutcome::Idempotent => ApplyResult::idempotent(),
    })
}

pub fn resolve_withdrawal_hold(
    withdrawal: &mut WithdrawalRecord,
    hold: &mut ReconciliationHoldRecord,
    resolution: HoldResolution,
) -> Result<ApplyResult, CoreError> {
    if hold.request != RequestReference::Withdrawal(withdrawal.id) {
        return Err(CoreError::HoldMismatch);
    }
    let expected_transfer = match &withdrawal.state {
        WithdrawalState::ReconciliationHold { transfer, .. }
        | WithdrawalState::ReleasePending { transfer, .. }
        | WithdrawalState::ReleaseTransferred { transfer, .. }
        | WithdrawalState::AcknowledgePending { transfer, .. }
        | WithdrawalState::Released { transfer, .. } => transfer,
        _ => return Err(CoreError::HoldMismatch),
    };
    if hold.transfer != *expected_transfer {
        return Err(CoreError::HoldMismatch);
    }

    let mut next_hold = hold.clone();
    let hold_outcome = next_hold.resolve(resolution)?;
    let mut next_withdrawal = withdrawal.clone();
    let (request_outcome, fee_delta) = match (resolution, &withdrawal.state) {
        (
            HoldResolution::Succeeded { ledger_block_index },
            WithdrawalState::ReconciliationHold {
                hold_id,
                transfer,
                settlement,
            },
        ) if *hold_id == hold.id => {
            next_withdrawal.state = WithdrawalState::ReleaseTransferred {
                transfer: transfer.clone(),
                settlement: *settlement,
                ledger_block_index,
                source_hold: Some(hold.id),
            };
            (ApplyOutcome::Applied, settlement.service_fee)
        }
        (
            HoldResolution::Succeeded { ledger_block_index },
            WithdrawalState::ReleaseTransferred {
                ledger_block_index: current,
                source_hold: Some(source_hold),
                ..
            }
            | WithdrawalState::AcknowledgePending {
                ledger_block_index: current,
                source_hold: Some(source_hold),
                ..
            }
            | WithdrawalState::Released {
                ledger_block_index: current,
                source_hold: Some(source_hold),
                ..
            },
        ) if *source_hold == hold.id && *current == ledger_block_index => {
            (ApplyOutcome::Idempotent, Amount::ZERO)
        }
        (
            HoldResolution::Absent {
                history_watermark: Some(_),
            },
            WithdrawalState::ReconciliationHold {
                hold_id,
                transfer,
                settlement,
            },
        ) if *hold_id == hold.id => {
            next_withdrawal.state = WithdrawalState::ReleasePending {
                transfer: transfer.clone(),
                settlement: *settlement,
            };
            (ApplyOutcome::Applied, Amount::ZERO)
        }
        (
            HoldResolution::Absent {
                history_watermark: Some(_),
            },
            WithdrawalState::ReleasePending { .. },
        ) => (ApplyOutcome::Idempotent, Amount::ZERO),
        (
            HoldResolution::Absent {
                history_watermark: None,
            },
            _,
        ) => {
            return Err(CoreError::MissingReconciliationEvidence);
        }
        _ => return Err(CoreError::HoldMismatch),
    };
    if hold_outcome != request_outcome {
        return Err(CoreError::ConflictingReplay);
    }

    *withdrawal = next_withdrawal;
    *hold = next_hold;
    Ok(match request_outcome {
        ApplyOutcome::Applied => ApplyResult::applied(fee_delta),
        ApplyOutcome::Idempotent => ApplyResult::idempotent(),
    })
}
