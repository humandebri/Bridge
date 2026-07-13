use candid::{CandidType, Deserialize, Principal};
use serde::Serialize;

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct FeeRecipientConfig {
    pub owner: Principal,
    pub subaccount: Vec<u8>,
}

pub const KINIC_LEDGER_CANISTER_ID: &str = "73mez-iiaaa-aaaaq-aaasq-cai";
pub const KINIC_INDEX_CANISTER_ID: &str = "7vojr-tyaaa-aaaaq-aaatq-cai";
pub const KINIC_DECIMALS: u8 = 8;
pub const BASE_MAINNET_CHAIN_ID: u64 = 8453;

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct BridgeInitArgs {
    pub ledger_canister_id: Principal,
    pub index_canister_id: Principal,
    pub evm_rpc_canister_id: Principal,
    pub custom_evm_rpc_urls: Vec<String>,
    pub base_chain_id: u64,
    pub bridge_contract: Vec<u8>,
    pub ecdsa_key_name: String,
    pub ecdsa_derivation_path: Vec<Vec<u8>>,
    pub poll_interval_seconds: u64,
    pub transaction_gas_limit: u128,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
    pub eth_floor_wei: u128,
    pub cycles_floor: u128,
    pub settlement_cycle_ceiling: u128,
    pub governance_principal: Principal,
    pub pause_principals: Vec<Principal>,
    pub finance_administrator: Principal,
    pub fee_recipient: FeeRecipientConfig,
}

impl BridgeInitArgs {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.bridge_contract.len() != 20 {
            return Err("bridge contract must be 20 bytes");
        }
        if self.base_chain_id == 0 || self.ecdsa_key_name.is_empty() {
            return Err("chain id and ECDSA key name must be configured");
        }
        if !self.custom_evm_rpc_urls.is_empty() && self.custom_evm_rpc_urls.len() != 3 {
            return Err("custom EVM RPC must configure exactly three providers");
        }
        if !(60..=300).contains(&self.poll_interval_seconds) {
            return Err("poll interval must be between 60 and 300 seconds");
        }
        if self.transaction_gas_limit == 0 || self.max_fee_per_gas == 0 {
            return Err("transaction gas limits must be nonzero");
        }
        if self.governance_principal == Principal::anonymous()
            || self.finance_administrator == Principal::anonymous()
            || self.pause_principals.is_empty()
            || self.pause_principals.len() > 10
            || self
                .pause_principals
                .iter()
                .any(|principal| *principal == Principal::anonymous())
            || self.fee_recipient.owner == Principal::anonymous()
            || !matches!(self.fee_recipient.subaccount.len(), 0 | 32)
        {
            return Err("administrator principals and fee recipient must be valid");
        }
        let mut principals = self.pause_principals.clone();
        principals.sort();
        principals.dedup();
        if principals.len() != self.pause_principals.len() {
            return Err("pause principals must be distinct");
        }
        Ok(())
    }

    pub const fn reserve_policy(&self) -> bridge_core::ReservePolicy {
        bridge_core::ReservePolicy {
            eth_floor_wei: self.eth_floor_wei,
            cycles_floor: self.cycles_floor,
            settlement_cycle_ceiling: self.settlement_cycle_ceiling,
            transaction_gas_limit: self.transaction_gas_limit,
            max_fee_per_gas: self.max_fee_per_gas,
        }
    }

    pub fn contract_array(&self) -> [u8; 20] {
        self.bridge_contract
            .as_slice()
            .try_into()
            .expect("validated bridge contract")
    }
}

pub fn kinic_ledger_canister_id() -> Principal {
    Principal::from_text(KINIC_LEDGER_CANISTER_ID)
        .expect("the fixed KINIC ledger canister ID must be valid")
}

pub fn kinic_index_canister_id() -> Principal {
    Principal::from_text(KINIC_INDEX_CANISTER_ID)
        .expect("the fixed KINIC index canister ID must be valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_kinic_canister_ids_are_valid_and_distinct() {
        assert_eq!(
            kinic_ledger_canister_id().to_text(),
            KINIC_LEDGER_CANISTER_ID
        );
        assert_eq!(kinic_index_canister_id().to_text(), KINIC_INDEX_CANISTER_ID);
        assert_ne!(kinic_ledger_canister_id(), kinic_index_canister_id());
    }
}
