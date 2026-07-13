use crate::{Amount, CoreError};

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeeKind {
    Deposit,
    Withdrawal,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccountingState {
    pub fee_reserve: Amount,
    pub confirmed_deposit_fees: Amount,
    pub confirmed_withdrawal_fees: Amount,
}

impl AccountingState {
    pub fn confirm_fee(&mut self, kind: FeeKind, amount: Amount) -> Result<(), CoreError> {
        let next_reserve = self.fee_reserve.checked_add(amount)?;
        let next_deposit = if kind == FeeKind::Deposit {
            self.confirmed_deposit_fees.checked_add(amount)?
        } else {
            self.confirmed_deposit_fees
        };
        let next_withdrawal = if kind == FeeKind::Withdrawal {
            self.confirmed_withdrawal_fees.checked_add(amount)?
        } else {
            self.confirmed_withdrawal_fees
        };
        self.fee_reserve = next_reserve;
        self.confirmed_deposit_fees = next_deposit;
        self.confirmed_withdrawal_fees = next_withdrawal;
        Ok(())
    }
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceCost {
    pub eth_wei: u128,
    pub cycles: u128,
}

impl ResourceCost {
    fn checked_add(self, other: Self) -> Result<Self, CoreError> {
        Ok(Self {
            eth_wei: self
                .eth_wei
                .checked_add(other.eth_wei)
                .ok_or(CoreError::ArithmeticOverflow)?,
            cycles: self
                .cycles
                .checked_add(other.cycles)
                .ok_or(CoreError::ArithmeticOverflow)?,
        })
    }

    fn fits_within(self, available: Self) -> bool {
        self.eth_wei <= available.eth_wei && self.cycles <= available.cycles
    }
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceBudget {
    pub available: ResourceCost,
    pub settlement_floor: ResourceCost,
    pub pending_settlements: ResourceCost,
}

impl ResourceBudget {
    pub fn ensure_deposit_can_reserve(self, deposit_cost: ResourceCost) -> Result<(), CoreError> {
        let required = self
            .settlement_floor
            .checked_add(self.pending_settlements)?
            .checked_add(deposit_cost)?;
        if !required.fits_within(self.available) {
            return Err(CoreError::InsufficientSettlementReserve);
        }
        Ok(())
    }
}
