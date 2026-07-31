use bridge_core::{DepositState, WithdrawalState};
use candid::{CandidType, Deserialize};

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepositPhase {
    EscrowedUnquoted,
    AuthorizationPending,
    AuthorizationAvailable,
    ExpiryReconciliation,
    Minted,
    FundingReconciliationHold,
    RefundPending,
    RefundReconciliationHold,
    Refunded,
    Cancelled,
}

impl From<&DepositState> for DepositPhase {
    fn from(state: &DepositState) -> Self {
        match state {
            DepositState::FundingPending => {
                unreachable!("funding attempts are not public deposit records")
            }
            DepositState::EscrowedUnquoted { .. } => Self::EscrowedUnquoted,
            DepositState::AuthorizationPending { .. } => Self::AuthorizationPending,
            DepositState::AuthorizationAvailable { .. } => Self::AuthorizationAvailable,
            DepositState::ExpiryReconciliation { .. } => Self::ExpiryReconciliation,
            DepositState::Minted { .. } => Self::Minted,
            DepositState::FundingReconciliationHold { .. } => Self::FundingReconciliationHold,
            DepositState::RefundPending { .. } => Self::RefundPending,
            DepositState::RefundReconciliationHold { .. } => Self::RefundReconciliationHold,
            DepositState::Refunded { .. } => Self::Refunded,
            DepositState::Cancelled { .. } => Self::Cancelled,
        }
    }
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum WithdrawalPhase {
    Observed,
    ReleasePending,
    Paid,
    ReconciliationHold,
}

impl From<&WithdrawalState> for WithdrawalPhase {
    fn from(state: &WithdrawalState) -> Self {
        match state {
            WithdrawalState::Observed => Self::Observed,
            WithdrawalState::ReleasePending { .. } => Self::ReleasePending,
            WithdrawalState::Paid { .. } => Self::Paid,
            WithdrawalState::ReconciliationHold { .. } => Self::ReconciliationHold,
        }
    }
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettlementState {
    Deposit(DepositPhase),
    Withdrawal(WithdrawalPhase),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_phase_variants_are_stable() {
        assert_eq!(
            DepositPhase::from(&DepositState::EscrowedUnquoted {
                ledger_block_index: 1,
            }),
            DepositPhase::EscrowedUnquoted
        );
        assert_eq!(
            WithdrawalPhase::from(&WithdrawalState::Observed),
            WithdrawalPhase::Observed
        );
    }
}
