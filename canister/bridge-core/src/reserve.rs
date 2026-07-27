use crate::CoreError;

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReservePolicy {
    pub eth_floor_wei: u128,
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
    pub reserved_mint_eth_wei: u128,
    pub candidate_mint_eth_wei: u128,
    pub eth_balance_wei: u128,
    pub cycles_balance: u128,
    pub required_eth_wei: u128,
    pub required_cycles: u128,
    pub eth_surplus_wei: u128,
    pub cycles_surplus: u128,
    pub sufficient: bool,
}

impl ReservePolicy {
    pub fn snapshot(
        self,
        nonterminal_withdrawals: u64,
        reserved_deposits: u64,
        candidate_deposits: u64,
        reserved_mint_eth_wei: u128,
        candidate_mint_eth_wei: u128,
        eth_balance_wei: u128,
        cycles_balance: u128,
    ) -> Result<ReserveSnapshot, CoreError> {
        let count = u128::from(nonterminal_withdrawals)
            .checked_add(u128::from(reserved_deposits))
            .and_then(|value| value.checked_add(u128::from(candidate_deposits)))
            .ok_or(CoreError::ArithmeticOverflow)?;
        let required_eth_wei = self
            .eth_floor_wei
            .checked_add(reserved_mint_eth_wei)
            .and_then(|value| value.checked_add(candidate_mint_eth_wei))
            .ok_or(CoreError::ArithmeticOverflow)?;
        let required_cycles =
            crate::checked_requirement(self.cycles_floor, self.settlement_cycle_ceiling, count)
                .ok_or(CoreError::ArithmeticOverflow)?;
        Ok(ReserveSnapshot {
            nonterminal_withdrawals,
            reserved_deposits,
            candidate_deposits,
            reserved_operation_count: count,
            reserved_mint_eth_wei,
            candidate_mint_eth_wei,
            eth_balance_wei,
            cycles_balance,
            required_eth_wei,
            required_cycles,
            eth_surplus_wei: eth_balance_wei.saturating_sub(required_eth_wei),
            cycles_surplus: cycles_balance.saturating_sub(required_cycles),
            sufficient: crate::resources_sufficient(
                eth_balance_wei,
                required_eth_wei,
                cycles_balance,
                required_cycles,
            ),
        })
    }
}
