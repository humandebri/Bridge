use crate::{Amount, CoreError, EvmOperationId, HoldId, LedgerTransferIdentity};

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerCallOutcome {
    Succeeded { block_index: u128 },
    Duplicate { block_index: u128 },
    DefinitiveFailure { code: LedgerFailure },
    RetryableFailure { code: LedgerFailure },
    Ambiguous,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerFailure {
    BadFee { expected_fee: Amount },
    BadBurn { minimum: Amount },
    InsufficientFunds { balance: Amount },
    InsufficientAllowance { allowance: Amount },
    TooOld,
    CreatedInFuture { ledger_time_ns: u64 },
    TemporarilyUnavailable,
    Generic { code: u64 },
}

impl LedgerCallOutcome {
    pub const fn confirmed_block(&self) -> Option<u128> {
        match self {
            Self::Succeeded { block_index } | Self::Duplicate { block_index } => Some(*block_index),
            Self::DefinitiveFailure { .. } | Self::RetryableFailure { .. } | Self::Ambiguous => {
                None
            }
        }
    }

    pub const fn requires_hold(&self) -> bool {
        matches!(self, Self::Ambiguous)
    }
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvmTransactionEnvelope {
    pub operation_id: EvmOperationId,
    pub operation_ids: Vec<EvmOperationId>,
    pub payload_hash: [u8; 32],
    pub nonce: u64,
    pub chain_id: u64,
    pub contract: [u8; 20],
    pub calldata: Vec<u8>,
    pub gas_limit: u128,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
    pub signed_transaction: Option<Vec<u8>>,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvmCallIntent {
    pub operation_id: EvmOperationId,
    pub payload_hash: [u8; 32],
    pub chain_id: u64,
    pub contract: [u8; 20],
    pub calldata: Vec<u8>,
    pub gas_limit: u128,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
}

impl EvmCallIntent {
    pub fn assign_nonce(self, nonce: u64) -> EvmTransactionEnvelope {
        EvmTransactionEnvelope {
            operation_id: self.operation_id,
            operation_ids: vec![self.operation_id],
            payload_hash: self.payload_hash,
            nonce,
            chain_id: self.chain_id,
            contract: self.contract,
            calldata: self.calldata,
            gas_limit: self.gas_limit,
            max_fee_per_gas: self.max_fee_per_gas,
            max_priority_fee_per_gas: self.max_priority_fee_per_gas,
            signed_transaction: None,
        }
    }
}

impl EvmTransactionEnvelope {
    pub fn validate(
        &self,
        expected_chain_id: u64,
        expected_contract: [u8; 20],
    ) -> Result<(), CoreError> {
        if self.chain_id != expected_chain_id || self.contract != expected_contract {
            return Err(CoreError::PayloadConflict);
        }
        if self.operation_ids.is_empty()
            || self.operation_ids.len() > 4
            || self.operation_ids[0] != self.operation_id
            || self.calldata.len() < 4
            || self.gas_limit == 0
            || self.max_fee_per_gas == 0
        {
            return Err(CoreError::InvalidAmount);
        }
        Ok(())
    }
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExternalProgress {
    pub nonce_initialized: bool,
    pub next_evm_nonce: u64,
    pub last_finalized_base_block: u64,
    pub last_finalized_mint_block: u64,
    pub last_eth_balance_wei: u128,
    pub reserve_sufficient: bool,
    pub last_reserve_observation_ns: u64,
    pub last_finalized_observation_ns: u64,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconciliationScanProgress {
    pub hold_id: HoldId,
    pub next_block: u128,
    pub ledger_tip: u128,
    pub index_watermark: u128,
    pub archives_complete: bool,
    pub matched_block: Option<u128>,
    pub transfer: LedgerTransferIdentity,
}

impl ReconciliationScanProgress {
    pub fn can_prove_absent(&self) -> bool {
        crate::scan_complete(
            self.next_block,
            self.ledger_tip,
            self.index_watermark,
            self.archives_complete,
            self.matched_block.is_some(),
        )
    }
}
