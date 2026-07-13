use crate::{
    Amount, ApplyOutcome, ApplyResult, CoreError, DepositId, DepositRecord, DepositState, HoldId,
    LedgerTransferIdentity, TransferAttempt, WithdrawalId, WithdrawalRecord, WithdrawalState,
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

    fn resolve_succeeded(&mut self, block: u128) -> Result<ApplyOutcome, CoreError> {
        match self.state {
            ReconciliationHoldState::Open => {
                self.state = ReconciliationHoldState::ResolvedSucceeded {
                    ledger_block_index: block,
                };
                Ok(ApplyOutcome::Applied)
            }
            ReconciliationHoldState::ResolvedSucceeded { ledger_block_index }
                if ledger_block_index == block =>
            {
                Ok(ApplyOutcome::Idempotent)
            }
            _ => Err(CoreError::ConflictingReplay),
        }
    }

    fn resolve_absent(&mut self, watermark: u128) -> Result<ApplyOutcome, CoreError> {
        match self.state {
            ReconciliationHoldState::Open => {
                self.state = ReconciliationHoldState::ResolvedAbsent {
                    history_watermark: watermark,
                };
                Ok(ApplyOutcome::Applied)
            }
            ReconciliationHoldState::ResolvedAbsent { history_watermark }
                if history_watermark == watermark =>
            {
                Ok(ApplyOutcome::Idempotent)
            }
            _ => Err(CoreError::ConflictingReplay),
        }
    }
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepositHoldResolution {
    Succeeded { ledger_block_index: u128 },
    Absent { history_watermark: u128 },
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WithdrawalHoldResolution {
    Succeeded {
        ledger_block_index: u128,
    },
    Absent {
        history_watermark: u128,
        next_identity: Box<LedgerTransferIdentity>,
    },
}

pub fn resolve_deposit_hold(
    deposit: &mut DepositRecord,
    hold: &mut ReconciliationHoldRecord,
    resolution: DepositHoldResolution,
) -> Result<ApplyResult, CoreError> {
    if !crate::evidence_matches(
        hold.request == RequestReference::Deposit(deposit.id),
        matches!(deposit.state, DepositState::ReconciliationHold { hold_id } if hold_id == hold.id)
            || matches!(
                deposit.state,
                DepositState::Escrowed { .. }
                    | DepositState::MintPending { .. }
                    | DepositState::Minted { .. }
                    | DepositState::Cancelled { .. }
            ),
        hold.transfer == deposit.transfer,
        true,
        true,
    ) {
        return Err(CoreError::HoldMismatch);
    }
    let mut next_hold = hold.clone();
    let mut next_deposit = deposit.clone();
    let (hold_outcome, request_outcome) = match resolution {
        DepositHoldResolution::Succeeded { ledger_block_index } => {
            let ho = next_hold.resolve_succeeded(ledger_block_index)?;
            let ro = match deposit.state {
                DepositState::ReconciliationHold { hold_id } if hold_id == hold.id => {
                    next_deposit.state = DepositState::Escrowed { ledger_block_index };
                    ApplyOutcome::Applied
                }
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
                } if current == ledger_block_index => ApplyOutcome::Idempotent,
                _ => return Err(CoreError::HoldMismatch),
            };
            (ho, ro)
        }
        DepositHoldResolution::Absent { history_watermark } => {
            let ho = next_hold.resolve_absent(history_watermark)?;
            let ro = match deposit.state {
                DepositState::ReconciliationHold { hold_id } if hold_id == hold.id => {
                    next_deposit.state = DepositState::Cancelled {
                        hold_id: Some(hold_id),
                        history_watermark: Some(history_watermark),
                        ledger_failure: None,
                    };
                    ApplyOutcome::Applied
                }
                DepositState::Cancelled {
                    hold_id: Some(hold_id),
                    history_watermark: Some(current),
                    ledger_failure: None,
                } if hold_id == hold.id && current == history_watermark => ApplyOutcome::Idempotent,
                _ => return Err(CoreError::HoldMismatch),
            };
            (ho, ro)
        }
    };
    if hold_outcome != request_outcome {
        return Err(CoreError::ConflictingReplay);
    }
    *deposit = next_deposit;
    *hold = next_hold;
    Ok(if request_outcome == ApplyOutcome::Applied {
        ApplyResult::applied(Amount::ZERO)
    } else {
        ApplyResult::idempotent()
    })
}

pub fn resolve_withdrawal_hold(
    withdrawal: &mut WithdrawalRecord,
    hold: &mut ReconciliationHoldRecord,
    resolution: WithdrawalHoldResolution,
) -> Result<ApplyResult, CoreError> {
    if hold.request != RequestReference::Withdrawal(withdrawal.id) {
        return Err(CoreError::HoldMismatch);
    }
    let held_attempt = match &withdrawal.state {
        WithdrawalState::ReconciliationHold {
            hold_id, attempt, ..
        } if *hold_id == hold.id => attempt,
        WithdrawalState::ReleaseTransferred {
            attempt,
            source_hold: Some(id),
            ..
        }
        | WithdrawalState::AcknowledgePending {
            attempt,
            source_hold: Some(id),
            ..
        }
        | WithdrawalState::AcknowledgeReverted {
            attempt,
            source_hold: Some(id),
            ..
        }
        | WithdrawalState::Released {
            attempt,
            source_hold: Some(id),
            ..
        } if *id == hold.id => attempt,
        WithdrawalState::ReleasePending { attempt, .. } if attempt.attempt_no > 0 => attempt,
        _ => return Err(CoreError::HoldMismatch),
    };
    if hold.transfer != held_attempt.identity && held_attempt.attempt_no == 0 {
        return Err(CoreError::HoldMismatch);
    }
    let mut next_hold = hold.clone();
    let mut next_withdrawal = withdrawal.clone();
    let (hold_outcome, request_outcome, fee_delta) = match resolution {
        WithdrawalHoldResolution::Succeeded { ledger_block_index } => {
            let ho = next_hold.resolve_succeeded(ledger_block_index)?;
            let ro = match &withdrawal.state {
                WithdrawalState::ReconciliationHold {
                    attempt,
                    settlement,
                    ..
                } => {
                    next_withdrawal.state = WithdrawalState::ReleaseTransferred {
                        attempt: attempt.clone(),
                        settlement: *settlement,
                        ledger_block_index,
                        source_hold: Some(hold.id),
                    };
                    ApplyOutcome::Applied
                }
                WithdrawalState::ReleaseTransferred {
                    ledger_block_index: current,
                    source_hold: Some(id),
                    ..
                }
                | WithdrawalState::AcknowledgePending {
                    ledger_block_index: current,
                    source_hold: Some(id),
                    ..
                }
                | WithdrawalState::AcknowledgeReverted {
                    ledger_block_index: current,
                    source_hold: Some(id),
                    ..
                }
                | WithdrawalState::Released {
                    ledger_block_index: current,
                    source_hold: Some(id),
                    ..
                } if *id == hold.id && *current == ledger_block_index => ApplyOutcome::Idempotent,
                _ => return Err(CoreError::HoldMismatch),
            };
            let fee = if ro == ApplyOutcome::Applied {
                match &withdrawal.state {
                    WithdrawalState::ReconciliationHold { settlement, .. } => {
                        settlement.service_fee
                    }
                    _ => Amount::ZERO,
                }
            } else {
                Amount::ZERO
            };
            (ho, ro, fee)
        }
        WithdrawalHoldResolution::Absent {
            history_watermark,
            next_identity,
        } => {
            let ho = next_hold.resolve_absent(history_watermark)?;
            let ro = match &withdrawal.state {
                WithdrawalState::ReconciliationHold {
                    attempt,
                    settlement,
                    ..
                } => {
                    let next_attempt: TransferAttempt =
                        attempt.retry_after_absence(*next_identity)?;
                    next_withdrawal.state = WithdrawalState::ReleasePending {
                        attempt: next_attempt,
                        settlement: *settlement,
                    };
                    ApplyOutcome::Applied
                }
                WithdrawalState::ReleasePending { attempt, .. }
                    if attempt.identity == *next_identity =>
                {
                    ApplyOutcome::Idempotent
                }
                _ => return Err(CoreError::HoldMismatch),
            };
            (ho, ro, Amount::ZERO)
        }
    };
    if hold_outcome != request_outcome {
        return Err(CoreError::ConflictingReplay);
    }
    *withdrawal = next_withdrawal;
    *hold = next_hold;
    Ok(if request_outcome == ApplyOutcome::Applied {
        ApplyResult::applied(fee_delta)
    } else {
        ApplyResult::idempotent()
    })
}
