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
    DepositFunding(DepositId),
    DepositRefund(DepositId),
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DepositHoldResolution {
    FundingSucceeded {
        funding_ledger_block_index: u128,
    },
    FundingAbsent {
        history_watermark: u128,
    },
    RefundSucceeded {
        refund_ledger_block_index: u128,
    },
    RefundAbsent {
        history_watermark: u128,
        next_identity: Box<LedgerTransferIdentity>,
    },
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WithdrawalHoldResolution {
    Succeeded {
        release_ledger_block_index: u128,
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
    let mut next_hold = hold.clone();
    let mut next_deposit = deposit.clone();
    let (hold_outcome, request_outcome) = match resolution {
        DepositHoldResolution::FundingSucceeded {
            funding_ledger_block_index,
        } => {
            if hold.request != RequestReference::DepositFunding(deposit.id)
                || hold.transfer != deposit.transfer
            {
                return Err(CoreError::HoldMismatch);
            }
            let current_funding = deposit.funding_ledger_block_index();
            let (next_funding, _) = crate::kernel::deposit_ledger_block_transition(
                current_funding,
                None,
                1,
                funding_ledger_block_index,
            )
            .ok_or(CoreError::LedgerBlockConflict)?;
            let next_funding = next_funding.ok_or(CoreError::LedgerBlockConflict)?;
            let ho = next_hold.resolve_succeeded(next_funding)?;
            let ro = match &deposit.state {
                DepositState::FundingReconciliationHold { hold_id } if *hold_id == hold.id => {
                    next_deposit.state = DepositState::EscrowedUnquoted {
                        funding_ledger_block_index: next_funding,
                    };
                    ApplyOutcome::Applied
                }
                _ if current_funding == Some(funding_ledger_block_index) => {
                    ApplyOutcome::Idempotent
                }
                _ => return Err(CoreError::HoldMismatch),
            };
            (ho, ro)
        }
        DepositHoldResolution::FundingAbsent { history_watermark } => {
            if hold.request != RequestReference::DepositFunding(deposit.id)
                || hold.transfer != deposit.transfer
            {
                return Err(CoreError::HoldMismatch);
            }
            let ho = next_hold.resolve_absent(history_watermark)?;
            let ro = match deposit.state {
                DepositState::FundingReconciliationHold { hold_id } if hold_id == hold.id => {
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
        DepositHoldResolution::RefundSucceeded {
            refund_ledger_block_index,
        } => {
            if hold.request != RequestReference::DepositRefund(deposit.id) {
                return Err(CoreError::HoldMismatch);
            }
            let current = match &deposit.state {
                DepositState::RefundReconciliationHold {
                    funding_ledger_block_index,
                    ..
                } => (Some(*funding_ledger_block_index), None),
                DepositState::Refunded {
                    funding_ledger_block_index,
                    refund_ledger_block_index,
                    ..
                } => (
                    Some(*funding_ledger_block_index),
                    Some(*refund_ledger_block_index),
                ),
                _ => (None, None),
            };
            let (_, next_refund) = crate::kernel::deposit_ledger_block_transition(
                current.0,
                current.1,
                2,
                refund_ledger_block_index,
            )
            .ok_or(CoreError::LedgerBlockConflict)?;
            let next_refund = next_refund.ok_or(CoreError::LedgerBlockConflict)?;
            let ho = next_hold.resolve_succeeded(next_refund)?;
            let ro = match &deposit.state {
                DepositState::RefundReconciliationHold {
                    reason,
                    funding_ledger_block_index,
                    hold_id,
                    attempt,
                } if *hold_id == hold.id && hold.transfer == attempt.identity => {
                    next_deposit.state = DepositState::Refunded {
                        reason: *reason,
                        funding_ledger_block_index: *funding_ledger_block_index,
                        attempt: attempt.clone(),
                        refund_ledger_block_index: next_refund,
                        source_hold: Some(hold.id),
                    };
                    ApplyOutcome::Applied
                }
                DepositState::Refunded {
                    refund_ledger_block_index: current,
                    source_hold: Some(id),
                    attempt,
                    ..
                } if *id == hold.id
                    && *current == refund_ledger_block_index
                    && hold.transfer == attempt.identity =>
                {
                    ApplyOutcome::Idempotent
                }
                _ => return Err(CoreError::HoldMismatch),
            };
            (ho, ro)
        }
        DepositHoldResolution::RefundAbsent {
            history_watermark,
            next_identity,
        } => {
            if hold.request != RequestReference::DepositRefund(deposit.id) {
                return Err(CoreError::HoldMismatch);
            }
            let ho = next_hold.resolve_absent(history_watermark)?;
            let ro = match &deposit.state {
                DepositState::RefundReconciliationHold {
                    reason,
                    funding_ledger_block_index,
                    hold_id,
                    attempt,
                } if *hold_id == hold.id && hold.transfer == attempt.identity => {
                    let next_attempt = attempt.retry_after_absence(*next_identity)?;
                    next_deposit.state = DepositState::RefundPending {
                        reason: *reason,
                        funding_ledger_block_index: *funding_ledger_block_index,
                        attempt: next_attempt,
                    };
                    ApplyOutcome::Applied
                }
                DepositState::RefundPending { attempt, .. }
                    if attempt.identity == *next_identity =>
                {
                    ApplyOutcome::Idempotent
                }
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
        WithdrawalState::Paid {
            attempt,
            source_hold: Some(id),
            ..
        } if *id == hold.id => attempt,
        WithdrawalState::ReleasePending { attempt, .. } if attempt.attempt_no > 0 => attempt,
        _ => return Err(CoreError::HoldMismatch),
    };
    // After an absence resolution the hold remains bound to the old transfer while the
    // withdrawal advances to a replacement attempt. Accept that shape only for replay;
    // the resolution branch below still requires the exact replacement identity.
    if hold.transfer != held_attempt.identity && held_attempt.attempt_no == 0 {
        return Err(CoreError::HoldMismatch);
    }
    let mut next_hold = hold.clone();
    let mut next_withdrawal = withdrawal.clone();
    let (hold_outcome, request_outcome, fee_delta) = match resolution {
        WithdrawalHoldResolution::Succeeded {
            release_ledger_block_index,
        } => {
            let current_release = match &withdrawal.state {
                WithdrawalState::Paid {
                    release_ledger_block_index,
                    ..
                } => Some(*release_ledger_block_index),
                _ => None,
            };
            let next_release = crate::kernel::withdrawal_ledger_block_transition(
                current_release,
                1,
                release_ledger_block_index,
            )
            .ok_or(CoreError::LedgerBlockConflict)?
            .ok_or(CoreError::LedgerBlockConflict)?;
            let ho = next_hold.resolve_succeeded(next_release)?;
            let ro = match &withdrawal.state {
                WithdrawalState::ReconciliationHold {
                    attempt,
                    settlement,
                    ..
                } => {
                    let (next_state, escrow_debit, reserve_credit, liability_debit) =
                        crate::kernel::withdrawal_transition_effects(
                            3,
                            4,
                            settlement.amount_out.get(),
                            settlement.ledger_fee.get(),
                            settlement.service_fee.get(),
                        )
                        .ok_or(CoreError::SettlementMismatch)?;
                    if next_state != 2
                        || Amount::new(escrow_debit)
                            != settlement.amount_out.checked_add(settlement.ledger_fee)?
                        || Amount::new(liability_debit)
                            != settlement.amount_out.checked_add(settlement.service_fee)?
                    {
                        return Err(CoreError::SettlementMismatch);
                    }
                    next_withdrawal.state = WithdrawalState::Paid {
                        attempt: attempt.clone(),
                        settlement: *settlement,
                        release_ledger_block_index: next_release,
                        source_hold: Some(hold.id),
                    };
                    (ApplyOutcome::Applied, Amount::new(reserve_credit))
                }
                WithdrawalState::Paid {
                    release_ledger_block_index: current,
                    source_hold: Some(id),
                    ..
                } if *id == hold.id && *current == release_ledger_block_index => {
                    (ApplyOutcome::Idempotent, Amount::ZERO)
                }
                _ => return Err(CoreError::HoldMismatch),
            };
            let (ro, fee) = ro;
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
                    let (next_state, escrow_debit, reserve_credit, liability_debit) =
                        crate::kernel::withdrawal_transition_effects(
                            3,
                            5,
                            settlement.amount_out.get(),
                            settlement.ledger_fee.get(),
                            settlement.service_fee.get(),
                        )
                        .ok_or(CoreError::SettlementMismatch)?;
                    if next_state != 1
                        || escrow_debit != 0
                        || reserve_credit != 0
                        || liability_debit != 0
                    {
                        return Err(CoreError::SettlementMismatch);
                    }
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
