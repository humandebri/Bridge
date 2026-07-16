use bridge_core::{DepositState, WithdrawalState};
use candid::{CandidType, Deserialize};

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepositPhase {
    PullPending,
    Escrowed,
    MintPending,
    Minted,
    MintReverted,
    ReconciliationHold,
    Cancelled,
}

impl From<&DepositState> for DepositPhase {
    fn from(state: &DepositState) -> Self {
        match state {
            DepositState::PullPending => Self::PullPending,
            DepositState::Escrowed { .. } => Self::Escrowed,
            DepositState::MintPending { .. } => Self::MintPending,
            DepositState::Minted { .. } => Self::Minted,
            DepositState::MintReverted { .. } => Self::MintReverted,
            DepositState::ReconciliationHold { .. } => Self::ReconciliationHold,
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
            DepositPhase::from(&DepositState::PullPending),
            DepositPhase::PullPending
        );
        assert_eq!(
            WithdrawalPhase::from(&WithdrawalState::Observed),
            WithdrawalPhase::Observed
        );
    }
}
