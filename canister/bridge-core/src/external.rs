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
        if self.calldata.len() < 4 || self.gas_limit == 0 || self.max_fee_per_gas == 0 {
            return Err(CoreError::InvalidAmount);
        }
        Ok(())
    }
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalizedObservationRecord {
    pub chain_id: u64,
    pub block_number: u64,
    pub block_hash: [u8; 32],
    pub observed_at_ns: u64,
    pub bridge_signer: [u8; 20],
    pub runtime_sha256: [u8; 32],
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
    pub reserve_observation_generation: u64,
    pub last_reserve_observation_ns: u64,
    pub last_finalized_observation_ns: u64,
    pub finalized_observation: Option<FinalizedObservationRecord>,
}

impl ExternalProgress {
    pub fn observe_finalized(
        &mut self,
        candidate: FinalizedObservationRecord,
    ) -> Result<(), CoreError> {
        if candidate.block_number < self.last_finalized_base_block {
            return Err(CoreError::StaleFinalizedObservation);
        }
        if let Some(current) = self.finalized_observation {
            if candidate.chain_id != current.chain_id {
                return Err(CoreError::ConflictingFinalizedObservation);
            }
            if candidate.block_number < current.block_number {
                return Err(CoreError::StaleFinalizedObservation);
            }
            if candidate.block_number == current.block_number
                && (candidate.block_hash != current.block_hash
                    || candidate.bridge_signer != current.bridge_signer
                    || candidate.runtime_sha256 != current.runtime_sha256)
            {
                return Err(CoreError::ConflictingFinalizedObservation);
            }
        }

        let observed_at_ns = self
            .last_finalized_observation_ns
            .max(candidate.observed_at_ns);
        self.last_finalized_base_block = candidate.block_number;
        self.last_finalized_observation_ns = observed_at_ns;
        self.finalized_observation = Some(FinalizedObservationRecord {
            observed_at_ns,
            ..candidate
        });
        Ok(())
    }
}

#[cfg(test)]
mod finalized_observation_tests {
    use super::*;

    fn observation(block_number: u64) -> FinalizedObservationRecord {
        FinalizedObservationRecord {
            chain_id: 8453,
            block_number,
            block_hash: [block_number as u8; 32],
            observed_at_ns: block_number * 10,
            bridge_signer: [7; 20],
            runtime_sha256: [8; 32],
        }
    }

    #[test]
    fn finalized_observations_advance_monotonically() {
        let mut progress = ExternalProgress::default();
        let first = observation(10);
        progress
            .observe_finalized(first)
            .expect("first observation");
        assert_eq!(progress.finalized_observation, Some(first));

        let next = observation(11);
        progress.observe_finalized(next).expect("newer observation");
        assert_eq!(progress.last_finalized_base_block, 11);
        assert_eq!(progress.finalized_observation, Some(next));
    }

    #[test]
    fn stale_finalized_observations_leave_progress_unchanged() {
        let current = observation(11);
        let mut progress = ExternalProgress::default();
        progress
            .observe_finalized(current)
            .expect("current observation");
        let before = progress;

        assert_eq!(
            progress.observe_finalized(observation(10)),
            Err(CoreError::StaleFinalizedObservation)
        );
        assert_eq!(progress, before);

        let mut block_only = ExternalProgress {
            last_finalized_base_block: 12,
            ..ExternalProgress::default()
        };
        let before = block_only;
        assert_eq!(
            block_only.observe_finalized(observation(11)),
            Err(CoreError::StaleFinalizedObservation)
        );
        assert_eq!(block_only, before);
    }

    #[test]
    fn repeated_finalized_observation_only_advances_time() {
        let mut progress = ExternalProgress::default();
        let first = observation(10);
        progress
            .observe_finalized(first)
            .expect("first observation");
        let repeated = FinalizedObservationRecord {
            observed_at_ns: first.observed_at_ns + 5,
            ..first
        };
        progress
            .observe_finalized(repeated)
            .expect("matching repeated observation");
        assert_eq!(progress.finalized_observation, Some(repeated));

        progress
            .observe_finalized(first)
            .expect("older timestamp for the same observation");
        assert_eq!(progress.finalized_observation, Some(repeated));
    }

    #[test]
    fn conflicting_finalized_identity_leaves_progress_unchanged() {
        let current = observation(10);
        for conflicting in [
            FinalizedObservationRecord {
                chain_id: current.chain_id + 1,
                ..current
            },
            FinalizedObservationRecord {
                block_hash: [9; 32],
                ..current
            },
            FinalizedObservationRecord {
                bridge_signer: [9; 20],
                ..current
            },
            FinalizedObservationRecord {
                runtime_sha256: [9; 32],
                ..current
            },
        ] {
            let mut progress = ExternalProgress::default();
            progress
                .observe_finalized(current)
                .expect("current observation");
            let before = progress;
            assert_eq!(
                progress.observe_finalized(conflicting),
                Err(CoreError::ConflictingFinalizedObservation)
            );
            assert_eq!(progress, before);
        }
    }
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReconciliationTarget {
    Hold(HoldId),
    FeePayout(u64),
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconciliationArchiveRange {
    pub canister_id: Vec<u8>,
    pub method: String,
    pub start: u128,
    pub length: u128,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconciliationLedgerPage {
    pub end: u128,
    pub archives: Vec<ReconciliationArchiveRange>,
    pub next_archive: u16,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReconciliationScanPhase {
    Ledger {
        next_block: u128,
        ledger_tip: Option<u128>,
        pending_page: Option<Box<ReconciliationLedgerPage>>,
    },
    Index {
        ledger_watermark: u128,
        index_watermark: Option<u128>,
        next_start: Option<u128>,
    },
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconciliationScanProgress {
    pub target: ReconciliationTarget,
    pub transfer: LedgerTransferIdentity,
    pub phase: ReconciliationScanPhase,
}

impl ReconciliationScanProgress {
    pub fn new(target: ReconciliationTarget, transfer: LedgerTransferIdentity) -> Self {
        Self {
            target,
            transfer,
            phase: ReconciliationScanPhase::Ledger {
                next_block: 0,
                ledger_tip: None,
                pending_page: None,
            },
        }
    }
}
