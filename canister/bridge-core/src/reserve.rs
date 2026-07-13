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
    pub transaction_gas_limit: u128,
    pub max_fee_per_gas: u128,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReserveSnapshot {
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
        eth_balance_wei: u128,
        cycles_balance: u128,
    ) -> Result<ReserveSnapshot, CoreError> {
        let count = u128::from(nonterminal_withdrawals);
        let per_settlement_eth =
            crate::checked_requirement(0, self.transaction_gas_limit, self.max_fee_per_gas)
                .ok_or(CoreError::ArithmeticOverflow)?;
        let required_eth_wei =
            crate::checked_requirement(self.eth_floor_wei, per_settlement_eth, count)
                .ok_or(CoreError::ArithmeticOverflow)?;
        let required_cycles =
            crate::checked_requirement(self.cycles_floor, self.settlement_cycle_ceiling, count)
                .ok_or(CoreError::ArithmeticOverflow)?;
        Ok(ReserveSnapshot {
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
