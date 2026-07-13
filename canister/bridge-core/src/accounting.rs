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

    pub fn spend_fee_reserve(&mut self, amount: Amount) -> Result<(), CoreError> {
        self.fee_reserve = self.fee_reserve.checked_sub(amount)?;
        Ok(())
    }
}
