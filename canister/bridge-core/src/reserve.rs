use crate::CoreError;

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReservePolicy {
    pub cycles_floor: u128,
    pub settlement_cycle_ceiling: u128,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReserveSnapshot {
    pub nonterminal_withdrawals: u64,
    pub reserved_deposits: u64,
    pub candidate_deposits: u64,
    pub reserved_operation_count: u128,
    pub cycles_balance: u128,
    pub required_cycles: u128,
    pub cycles_surplus: u128,
    pub sufficient: bool,
}

impl ReservePolicy {
    pub fn required_cycles(
        self,
        nonterminal_withdrawals: u64,
        reserved_deposits: u64,
        candidate_deposits: u64,
    ) -> Result<u128, CoreError> {
        let count = u128::from(nonterminal_withdrawals)
            .checked_add(u128::from(reserved_deposits))
            .and_then(|value| value.checked_add(u128::from(candidate_deposits)))
            .ok_or(CoreError::ArithmeticOverflow)?;
        crate::checked_requirement(self.cycles_floor, self.settlement_cycle_ceiling, count)
            .ok_or(CoreError::ArithmeticOverflow)
    }

    pub fn snapshot(
        self,
        nonterminal_withdrawals: u64,
        reserved_deposits: u64,
        candidate_deposits: u64,
        cycles_balance: u128,
    ) -> Result<ReserveSnapshot, CoreError> {
        let count = u128::from(nonterminal_withdrawals)
            .checked_add(u128::from(reserved_deposits))
            .and_then(|value| value.checked_add(u128::from(candidate_deposits)))
            .ok_or(CoreError::ArithmeticOverflow)?;
        let required_cycles = self.required_cycles(
            nonterminal_withdrawals,
            reserved_deposits,
            candidate_deposits,
        )?;
        Ok(ReserveSnapshot {
            nonterminal_withdrawals,
            reserved_deposits,
            candidate_deposits,
            reserved_operation_count: count,
            cycles_balance,
            required_cycles,
            cycles_surplus: cycles_balance.saturating_sub(required_cycles),
            sufficient: cycles_balance >= required_cycles,
        })
    }
}
