use crate::config::BridgeInitArgs;
use crate::{admin::AdminState, config::FeeRecipientConfig};
use bridge_core::{
    resolve_deposit_hold, resolve_withdrawal_hold, AccountingState, ApplyResult, CoreError,
    DepositHoldResolution, DepositId, DepositRecord, EvmCallIntent, EvmOperationRecord,
    EvmOperationState, EvmSafeObservation, EvmTransactionEnvelope, ExternalProgress, HoldId,
    ReconciliationHoldRecord, ReconciliationHoldState, ReconciliationScanProgress,
    WithdrawalHoldResolution, WithdrawalId, WithdrawalRecord, WithdrawalState,
};
use candid::{CandidType, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::{Bound, Storable},
    Memory, StableBTreeMap, StableCell,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{borrow::Cow, fmt, io::Cursor};

pub const SCHEMA_VERSION: u16 = 5;
const WIRE_VERSION: u8 = 1;
const MAX_STABLE_VALUE_BYTES: usize = 16 * 1024;

const SCHEMA_MEMORY_ID: MemoryId = MemoryId::new(0);
const ACCOUNTING_MEMORY_ID: MemoryId = MemoryId::new(1);
const DEPOSITS_MEMORY_ID: MemoryId = MemoryId::new(2);
const WITHDRAWALS_MEMORY_ID: MemoryId = MemoryId::new(3);
const EVM_OPERATIONS_MEMORY_ID: MemoryId = MemoryId::new(4);
const RECONCILIATION_HOLDS_MEMORY_ID: MemoryId = MemoryId::new(5);
const COUNTERS_MEMORY_ID: MemoryId = MemoryId::new(6);
const EXTERNAL_PROGRESS_MEMORY_ID: MemoryId = MemoryId::new(7);
const EVM_ENVELOPES_MEMORY_ID: MemoryId = MemoryId::new(8);
const RECONCILIATION_SCANS_MEMORY_ID: MemoryId = MemoryId::new(9);
const CONFIG_MEMORY_ID: MemoryId = MemoryId::new(10);
const DEPOSIT_INTENTS_MEMORY_ID: MemoryId = MemoryId::new(11);
const ADMIN_STATE_MEMORY_ID: MemoryId = MemoryId::new(12);
const AUDIT_EVENTS_MEMORY_ID: MemoryId = MemoryId::new(13);
const FEE_PAYOUTS_MEMORY_ID: MemoryId = MemoryId::new(14);
const EVM_CALL_INTENTS_MEMORY_ID: MemoryId = MemoryId::new(15);
const EVM_SAFE_OBSERVATIONS_MEMORY_ID: MemoryId = MemoryId::new(16);
pub const RESERVED_MEMORY_IDS: core::ops::RangeInclusive<u8> = 17..=31;

type StableMemory<M> = VirtualMemory<M>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StableBlob(Vec<u8>);

impl StableBlob {
    pub fn new(bytes: Vec<u8>) -> Result<Self, StorageError> {
        if bytes.len() > MAX_STABLE_VALUE_BYTES {
            return Err(StorageError::ValueTooLarge {
                actual: bytes.len(),
                maximum: MAX_STABLE_VALUE_BYTES,
            });
        }
        Ok(Self(bytes))
    }

    fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl Storable for StableBlob {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.0)
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        Self(bytes.into_owned())
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: MAX_STABLE_VALUE_BYTES as u32,
        is_fixed_size: false,
    };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterState {
    pub next_evm_operation_id: u64,
    pub next_hold_id: u64,
    pub pending_evm_operations: u64,
    pub reconciliation_holds: u64,
    #[serde(default)]
    pub pending_ledger_operations: u64,
    #[serde(default)]
    pub next_audit_sequence: u64,
    #[serde(default)]
    pub next_fee_payout_id: u64,
    pub reserved_deposit_mint_amount: u128,
    pub reverted_evm_operations: u64,
}

#[derive(CandidType, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditEventKind {
    DepositsPaused,
    DepositsPauseRepeated,
    DepositsResumed,
    RuntimeAdministratorsRotated,
    FeeRecipientChanged {
        previous: FeeRecipientConfig,
        current: FeeRecipientConfig,
    },
    ReserveGateChanged {
        sufficient: bool,
    },
    FeePayoutRequested {
        amount: u128,
    },
    BaseServiceFeeChanged {
        previous: Option<u128>,
        current: u128,
    },
    EvmOperationReverted {
        operation_id: u64,
        kind: AuditedEvmOperationKind,
        transaction_hash: Vec<u8>,
        finalized_block_number: u64,
    },
}

#[derive(CandidType, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditedEvmOperationKind {
    MintDeposit,
    AcknowledgeRelease,
    RefundWithdrawal,
}

impl From<bridge_core::EvmOperationKind> for AuditedEvmOperationKind {
    fn from(value: bridge_core::EvmOperationKind) -> Self {
        match value {
            bridge_core::EvmOperationKind::MintDeposit => Self::MintDeposit,
            bridge_core::EvmOperationKind::AcknowledgeRelease => Self::AcknowledgeRelease,
            bridge_core::EvmOperationKind::RefundWithdrawal => Self::RefundWithdrawal,
        }
    }
}

#[derive(CandidType, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub sequence: u64,
    pub timestamp_ns: u64,
    pub caller: Principal,
    pub kind: AuditEventKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageCounts {
    pub deposits: u64,
    pub withdrawals: u64,
    pub pending_evm_operations: u64,
    pub reconciliation_holds: u64,
    pub pending_ledger_operations: u64,
    pub reserved_deposit_mint_amount: u128,
    pub reverted_evm_operations: u64,
    pub withdrawal_log_cursor: u64,
    pub last_finalized_base_block: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepositIntent {
    pub deposit_id: [u8; 32],
    pub caller: Vec<u8>,
    pub client_request_id: [u8; 32],
    pub base_recipient: [u8; 20],
    pub payload_hash: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageError {
    EncodeFailed,
    DecodeFailed,
    MissingWireVersion,
    UnsupportedWireVersion(u8),
    UnsupportedSchemaVersion(u16),
    ValueTooLarge { actual: usize, maximum: usize },
    CounterOverflow,
    CounterUnderflow,
    RecordNotFound,
    Core(CoreError),
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for StorageError {}

impl From<CoreError> for StorageError {
    fn from(error: CoreError) -> Self {
        Self::Core(error)
    }
}

pub struct StableStore<M: Memory> {
    schema: StableCell<u16, StableMemory<M>>,
    accounting: StableCell<StableBlob, StableMemory<M>>,
    deposits: StableBTreeMap<[u8; 32], StableBlob, StableMemory<M>>,
    withdrawals: StableBTreeMap<[u8; 32], StableBlob, StableMemory<M>>,
    evm_operations: StableBTreeMap<u64, StableBlob, StableMemory<M>>,
    reconciliation_holds: StableBTreeMap<u64, StableBlob, StableMemory<M>>,
    counters: StableCell<StableBlob, StableMemory<M>>,
    external_progress: StableCell<StableBlob, StableMemory<M>>,
    evm_envelopes: StableBTreeMap<u64, StableBlob, StableMemory<M>>,
    reconciliation_scans: StableBTreeMap<u64, StableBlob, StableMemory<M>>,
    config: StableCell<StableBlob, StableMemory<M>>,
    deposit_intents: StableBTreeMap<[u8; 32], StableBlob, StableMemory<M>>,
    admin_state: StableCell<StableBlob, StableMemory<M>>,
    audit_events: StableBTreeMap<u64, StableBlob, StableMemory<M>>,
    fee_payouts: StableBTreeMap<u64, StableBlob, StableMemory<M>>,
    evm_call_intents: StableBTreeMap<u64, StableBlob, StableMemory<M>>,
    evm_safe_observations: StableBTreeMap<u64, StableBlob, StableMemory<M>>,
}

impl<M: Memory> StableStore<M> {
    pub fn init(memory: M) -> Result<Self, StorageError> {
        let manager = MemoryManager::init(memory);
        let schema = StableCell::init(manager.get(SCHEMA_MEMORY_ID), SCHEMA_VERSION);
        if *schema.get() != SCHEMA_VERSION {
            return Err(StorageError::UnsupportedSchemaVersion(*schema.get()));
        }
        Ok(Self {
            schema,
            accounting: StableCell::init(
                manager.get(ACCOUNTING_MEMORY_ID),
                encode(&AccountingState::default())?,
            ),
            deposits: StableBTreeMap::init(manager.get(DEPOSITS_MEMORY_ID)),
            withdrawals: StableBTreeMap::init(manager.get(WITHDRAWALS_MEMORY_ID)),
            evm_operations: StableBTreeMap::init(manager.get(EVM_OPERATIONS_MEMORY_ID)),
            reconciliation_holds: StableBTreeMap::init(manager.get(RECONCILIATION_HOLDS_MEMORY_ID)),
            counters: StableCell::init(
                manager.get(COUNTERS_MEMORY_ID),
                encode(&CounterState::default())?,
            ),
            external_progress: StableCell::init(
                manager.get(EXTERNAL_PROGRESS_MEMORY_ID),
                encode(&ExternalProgress::default())?,
            ),
            evm_envelopes: StableBTreeMap::init(manager.get(EVM_ENVELOPES_MEMORY_ID)),
            reconciliation_scans: StableBTreeMap::init(manager.get(RECONCILIATION_SCANS_MEMORY_ID)),
            config: StableCell::init(
                manager.get(CONFIG_MEMORY_ID),
                encode(&Option::<BridgeInitArgs>::None)?,
            ),
            deposit_intents: StableBTreeMap::init(manager.get(DEPOSIT_INTENTS_MEMORY_ID)),
            admin_state: StableCell::init(
                manager.get(ADMIN_STATE_MEMORY_ID),
                encode(&Option::<AdminState>::None)?,
            ),
            audit_events: StableBTreeMap::init(manager.get(AUDIT_EVENTS_MEMORY_ID)),
            fee_payouts: StableBTreeMap::init(manager.get(FEE_PAYOUTS_MEMORY_ID)),
            evm_call_intents: StableBTreeMap::init(manager.get(EVM_CALL_INTENTS_MEMORY_ID)),
            evm_safe_observations: StableBTreeMap::init(
                manager.get(EVM_SAFE_OBSERVATIONS_MEMORY_ID),
            ),
        })
    }

    pub fn schema_version(&self) -> u16 {
        *self.schema.get()
    }

    pub fn accounting(&self) -> Result<AccountingState, StorageError> {
        decode(self.accounting.get())
    }

    pub fn set_accounting(&mut self, value: &AccountingState) -> Result<(), StorageError> {
        self.accounting.set(encode(value)?);
        Ok(())
    }

    pub fn counters(&self) -> Result<CounterState, StorageError> {
        decode(self.counters.get())
    }

    pub fn external_progress(&self) -> Result<ExternalProgress, StorageError> {
        decode(self.external_progress.get())
    }

    pub fn config(&self) -> Result<Option<BridgeInitArgs>, StorageError> {
        decode(self.config.get())
    }

    pub fn set_config_once(&mut self, value: &BridgeInitArgs) -> Result<(), StorageError> {
        match self.config()? {
            None => {
                self.config.set(encode(&Some(value.clone()))?);
                Ok(())
            }
            Some(previous) if previous == *value => Ok(()),
            Some(_) => Err(StorageError::Core(CoreError::ConflictingReplay)),
        }
    }

    pub fn initialize_admin(&mut self, config: &BridgeInitArgs) -> Result<(), StorageError> {
        if decode::<Option<AdminState>>(self.admin_state.get())?.is_some() {
            return Ok(());
        }
        let state = AdminState {
            deposits_paused: false,
            pause_principals: config.pause_principals.clone(),
            finance_administrator: config.finance_administrator,
            governance_principal: config.governance_principal,
            fee_recipient: config.fee_recipient.clone(),
        };
        self.admin_state.set(encode(&Some(state))?);
        Ok(())
    }

    pub fn admin_state(&self) -> Result<AdminState, StorageError> {
        decode::<Option<AdminState>>(self.admin_state.get())?.ok_or(StorageError::RecordNotFound)
    }
    pub fn set_admin_state(&mut self, value: &AdminState) -> Result<(), StorageError> {
        self.admin_state.set(encode(&Some(value.clone()))?);
        Ok(())
    }
    pub fn append_audit_event(
        &mut self,
        caller: Principal,
        kind: AuditEventKind,
    ) -> Result<AuditEvent, StorageError> {
        let mut counters = self.counters()?;
        let sequence = counters.next_audit_sequence;
        counters.next_audit_sequence =
            bridge_core::audit_next(sequence).ok_or(StorageError::CounterOverflow)?;
        let event = AuditEvent {
            sequence,
            timestamp_ns: ic_cdk::api::time(),
            caller,
            kind,
        };
        self.audit_events.insert(sequence, encode(&event)?);
        self.counters.set(encode(&counters)?);
        Ok(event)
    }
    pub fn audit_events(&self, start: u64, limit: u16) -> Result<Vec<AuditEvent>, StorageError> {
        self.audit_events
            .range(start..)
            .take(usize::from(limit))
            .map(|entry| decode(&entry.value()))
            .collect()
    }
    pub fn last_audit_sequence(&self) -> Result<Option<u64>, StorageError> {
        Ok(self
            .audit_events
            .iter()
            .next_back()
            .map(|entry| *entry.key()))
    }
    pub fn allocate_fee_payout_id(&mut self) -> Result<u64, StorageError> {
        let mut counters = self.counters()?;
        let id = counters.next_fee_payout_id;
        counters.next_fee_payout_id = id.checked_add(1).ok_or(StorageError::CounterOverflow)?;
        self.counters.set(encode(&counters)?);
        Ok(id)
    }
    pub fn put_fee_payout(
        &mut self,
        value: &crate::admin::FeePayoutRecord,
    ) -> Result<(), StorageError> {
        self.fee_payouts.insert(value.id, encode(value)?);
        Ok(())
    }
    pub fn pending_fee_payout_amount(&self) -> Result<u128, StorageError> {
        let mut total = 0u128;
        for entry in self.fee_payouts.iter() {
            let value: crate::admin::FeePayoutRecord = decode(&entry.value())?;
            if matches!(
                value.state,
                crate::admin::FeePayoutState::Pending
                    | crate::admin::FeePayoutState::ReconciliationHold
            ) {
                total = total
                    .checked_add(
                        value
                            .amount
                            .checked_add(value.transfer.fee.get())
                            .ok_or(StorageError::CounterOverflow)?,
                    )
                    .ok_or(StorageError::CounterOverflow)?;
            }
        }
        Ok(total)
    }
    pub fn first_held_fee_payout(
        &self,
    ) -> Result<Option<crate::admin::FeePayoutRecord>, StorageError> {
        for entry in self.fee_payouts.iter() {
            let value: crate::admin::FeePayoutRecord = decode(&entry.value())?;
            if matches!(
                value.state,
                crate::admin::FeePayoutState::ReconciliationHold
            ) {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    pub fn set_external_progress(&mut self, value: &ExternalProgress) -> Result<(), StorageError> {
        self.external_progress.set(encode(value)?);
        Ok(())
    }

    pub fn put_evm_envelope(&mut self, value: &EvmTransactionEnvelope) -> Result<(), StorageError> {
        if let Some(previous) = self.evm_envelope(value.operation_id.get())? {
            if previous != *value {
                let mut expected = value.clone();
                expected.signed_transaction = previous.signed_transaction.clone();
                if expected != previous || previous.signed_transaction.is_some() {
                    return Err(StorageError::Core(CoreError::ConflictingReplay));
                }
            }
        }
        self.evm_envelopes
            .insert(value.operation_id.get(), encode(value)?);
        Ok(())
    }

    pub fn evm_envelope(&self, id: u64) -> Result<Option<EvmTransactionEnvelope>, StorageError> {
        self.evm_envelopes
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }

    pub fn first_prepared_evm(
        &self,
    ) -> Result<Option<(EvmOperationRecord, EvmTransactionEnvelope)>, StorageError> {
        for entry in self.evm_operations.iter() {
            let operation: EvmOperationRecord = decode(&entry.value())?;
            if matches!(operation.state, EvmOperationState::Prepared) {
                let envelope = self
                    .evm_envelope(operation.id.get())?
                    .ok_or(StorageError::RecordNotFound)?;
                return Ok(Some((operation, envelope)));
            }
        }
        Ok(None)
    }
    pub fn put_evm_call_intent(&mut self, value: &EvmCallIntent) -> Result<(), StorageError> {
        if let Some(previous) = self.evm_call_intent(value.operation_id.get())? {
            if previous != *value {
                return Err(StorageError::Core(CoreError::ConflictingReplay));
            }
        }
        self.evm_call_intents
            .insert(value.operation_id.get(), encode(value)?);
        Ok(())
    }
    pub fn evm_call_intent(&self, id: u64) -> Result<Option<EvmCallIntent>, StorageError> {
        self.evm_call_intents
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }
    pub fn first_queued_evm(
        &self,
    ) -> Result<Option<(EvmOperationRecord, EvmCallIntent)>, StorageError> {
        let mut selected: Option<(u8, EvmOperationRecord, EvmCallIntent)> = None;
        for entry in self.evm_operations.iter() {
            let operation: EvmOperationRecord = decode(&entry.value())?;
            if !matches!(operation.state, EvmOperationState::Queued) {
                continue;
            }
            let priority = operation.kind.scheduler_priority();
            let intent = self
                .evm_call_intent(operation.id.get())?
                .ok_or(StorageError::RecordNotFound)?;
            if selected
                .as_ref()
                .map(|(p, o, _)| {
                    bridge_core::candidate_precedes(priority, operation.id.get(), *p, o.id.get())
                })
                .unwrap_or(true)
            {
                selected = Some((priority, operation, intent));
            }
        }
        Ok(selected.map(|(_, operation, intent)| (operation, intent)))
    }
    pub fn queued_evm_count(&self) -> Result<u64, StorageError> {
        let mut count = 0u64;
        for entry in self.evm_operations.iter() {
            let operation: EvmOperationRecord = decode(&entry.value())?;
            if matches!(operation.state, EvmOperationState::Queued) {
                count = count.checked_add(1).ok_or(StorageError::CounterOverflow)?;
            }
        }
        Ok(count)
    }

    pub fn put_reconciliation_scan(
        &mut self,
        value: &ReconciliationScanProgress,
    ) -> Result<(), StorageError> {
        if let Some(previous) = self.reconciliation_scan(value.hold_id.get())? {
            if previous.transfer != value.transfer || previous.hold_id != value.hold_id {
                return Err(StorageError::Core(CoreError::ConflictingReplay));
            }
        }
        self.reconciliation_scans
            .insert(value.hold_id.get(), encode(value)?);
        Ok(())
    }

    pub fn reconciliation_scan(
        &self,
        id: u64,
    ) -> Result<Option<ReconciliationScanProgress>, StorageError> {
        self.reconciliation_scans
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }

    #[cfg(test)]
    fn set_counters(&mut self, value: &CounterState) -> Result<(), StorageError> {
        self.counters.set(encode(value)?);
        Ok(())
    }

    pub fn put_deposit(&mut self, value: &DepositRecord) -> Result<(), StorageError> {
        let previous = self.deposit(value.id.bytes())?;
        let mut counters = self.counters()?;
        counters.pending_ledger_operations = adjust_active_count(
            counters.pending_ledger_operations,
            previous
                .as_ref()
                .map(is_pending_deposit_ledger)
                .unwrap_or(false),
            is_pending_deposit_ledger(value),
        )?;
        counters.reserved_deposit_mint_amount = adjust_reserved_mint_amount(
            counters.reserved_deposit_mint_amount,
            previous.as_ref(),
            value,
        )?;
        self.deposits.insert(value.id.bytes(), encode(value)?);
        self.counters.set(encode(&counters)?);
        Ok(())
    }

    pub fn put_deposit_intent(&mut self, value: &DepositIntent) -> Result<(), StorageError> {
        if let Some(previous) = self.deposit_intent(value.deposit_id)? {
            if previous == *value {
                return Ok(());
            }
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        self.deposit_intents
            .insert(value.deposit_id, encode(value)?);
        Ok(())
    }

    pub fn deposit_intent(&self, id: [u8; 32]) -> Result<Option<DepositIntent>, StorageError> {
        self.deposit_intents
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }

    pub fn allocate_hold_id(&mut self) -> Result<HoldId, StorageError> {
        let mut counters = self.counters()?;
        let id = counters.next_hold_id;
        counters.next_hold_id = id.checked_add(1).ok_or(StorageError::CounterOverflow)?;
        self.counters.set(encode(&counters)?);
        Ok(HoldId::new(id))
    }

    pub fn allocate_evm_operation_id(
        &mut self,
    ) -> Result<bridge_core::EvmOperationId, StorageError> {
        let mut counters = self.counters()?;
        let id = counters.next_evm_operation_id;
        counters.next_evm_operation_id = id.checked_add(1).ok_or(StorageError::CounterOverflow)?;
        self.counters.set(encode(&counters)?);
        Ok(bridge_core::EvmOperationId::new(id))
    }

    pub fn deposit(&self, id: [u8; 32]) -> Result<Option<DepositRecord>, StorageError> {
        self.deposits.get(&id).map(|blob| decode(&blob)).transpose()
    }

    pub fn first_pull_pending(&self) -> Result<Option<DepositRecord>, StorageError> {
        for entry in self.deposits.iter() {
            let record: DepositRecord = decode(&entry.value())?;
            if matches!(record.state, bridge_core::DepositState::PullPending) {
                return Ok(Some(record));
            }
        }
        Ok(None)
    }

    pub fn put_withdrawal(&mut self, value: &WithdrawalRecord) -> Result<(), StorageError> {
        let previous = self.withdrawal(value.id.bytes())?;
        let mut counters = self.counters()?;
        counters.pending_ledger_operations = adjust_active_count(
            counters.pending_ledger_operations,
            previous
                .as_ref()
                .map(is_pending_withdrawal_ledger)
                .unwrap_or(false),
            is_pending_withdrawal_ledger(value),
        )?;
        self.withdrawals.insert(value.id.bytes(), encode(value)?);
        self.counters.set(encode(&counters)?);
        Ok(())
    }

    pub fn put_refund_if_absent(
        &mut self,
        withdrawal: &WithdrawalRecord,
        operation: &EvmOperationRecord,
        intent: &EvmCallIntent,
    ) -> Result<bool, StorageError> {
        if self.withdrawal(withdrawal.id.bytes())?.is_some() {
            return Ok(false);
        }
        self.put_evm_call_intent(intent)?;
        self.put_evm_operation(operation)?;
        self.put_withdrawal(withdrawal)?;
        Ok(true)
    }

    pub fn withdrawal(&self, id: [u8; 32]) -> Result<Option<WithdrawalRecord>, StorageError> {
        self.withdrawals
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }

    pub fn first_release_pending(&self) -> Result<Option<WithdrawalRecord>, StorageError> {
        for entry in self.withdrawals.iter() {
            let blob = entry.value();
            let record: WithdrawalRecord = decode(&blob)?;
            if matches!(record.state, WithdrawalState::ReleasePending { .. }) {
                return Ok(Some(record));
            }
        }
        Ok(None)
    }

    pub fn nonterminal_withdrawal_count(&self) -> Result<u64, StorageError> {
        let mut count = 0u64;
        for entry in self.withdrawals.iter() {
            let record: WithdrawalRecord = decode(&entry.value())?;
            if !matches!(
                record.state,
                WithdrawalState::Released { .. } | WithdrawalState::Refunded { .. }
            ) {
                count = count.checked_add(1).ok_or(StorageError::CounterOverflow)?;
            }
        }
        Ok(count)
    }

    pub fn deposit_for_operation(
        &self,
        operation_id: bridge_core::EvmOperationId,
    ) -> Result<Option<DepositRecord>, StorageError> {
        for entry in self.deposits.iter() {
            let record: DepositRecord = decode(&entry.value())?;
            if matches!(
                record.state,
                bridge_core::DepositState::MintPending { operation_id: current, .. }
                    | bridge_core::DepositState::Minted { operation_id: current, .. }
                    | bridge_core::DepositState::MintReverted { operation_id: current, .. }
                    if current == operation_id
            ) {
                return Ok(Some(record));
            }
        }
        Ok(None)
    }

    pub fn withdrawal_for_operation(
        &self,
        operation_id: bridge_core::EvmOperationId,
    ) -> Result<Option<WithdrawalRecord>, StorageError> {
        for entry in self.withdrawals.iter() {
            let record: WithdrawalRecord = decode(&entry.value())?;
            if matches!(
                record.state,
                WithdrawalState::AcknowledgePending { operation_id: current, .. }
                    | WithdrawalState::Released { operation_id: current, .. }
                    | WithdrawalState::AcknowledgeReverted { operation_id: current, .. }
                    | WithdrawalState::RefundPending { operation_id: current, .. }
                    | WithdrawalState::Refunded { operation_id: current }
                    | WithdrawalState::RefundReverted { operation_id: current, .. }
                    if current == operation_id
            ) {
                return Ok(Some(record));
            }
        }
        Ok(None)
    }

    pub fn first_submitted_evm(&self) -> Result<Option<EvmOperationRecord>, StorageError> {
        for entry in self.evm_operations.iter() {
            let operation: EvmOperationRecord = decode(&entry.value())?;
            if matches!(operation.state, EvmOperationState::Submitted { .. }) {
                return Ok(Some(operation));
            }
        }
        Ok(None)
    }

    pub fn first_submitted_without_safe_observation(
        &self,
    ) -> Result<Option<(EvmOperationRecord, EvmTransactionEnvelope)>, StorageError> {
        for entry in self.evm_operations.iter() {
            let operation: EvmOperationRecord = decode(&entry.value())?;
            if !matches!(operation.state, EvmOperationState::Submitted { .. })
                || self.evm_safe_observation(operation.id.get())?.is_some()
            {
                continue;
            }
            let envelope = self
                .evm_envelope(operation.id.get())?
                .ok_or(StorageError::RecordNotFound)?;
            return Ok(Some((operation, envelope)));
        }
        Ok(None)
    }

    pub fn put_evm_operation(&mut self, value: &EvmOperationRecord) -> Result<(), StorageError> {
        let encoded_value = encode(value)?;
        let previous = self
            .evm_operations
            .get(&value.id.get())
            .map(|blob| decode::<EvmOperationRecord>(&blob))
            .transpose()?;
        let mut counters = self.counters()?;
        counters.pending_evm_operations = adjust_active_count(
            counters.pending_evm_operations,
            previous.as_ref().map(is_pending_evm).unwrap_or(false),
            is_pending_evm(value),
        )?;
        counters.reverted_evm_operations = adjust_active_count(
            counters.reverted_evm_operations,
            previous.as_ref().map(is_reverted_evm).unwrap_or(false),
            is_reverted_evm(value),
        )?;
        let encoded_counters = encode(&counters)?;
        self.evm_operations.insert(value.id.get(), encoded_value);
        self.counters.set(encoded_counters);
        Ok(())
    }

    pub fn evm_operation(&self, id: u64) -> Result<Option<EvmOperationRecord>, StorageError> {
        self.evm_operations
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }

    pub fn evm_safe_observation(
        &self,
        id: u64,
    ) -> Result<Option<EvmSafeObservation>, StorageError> {
        self.evm_safe_observations
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }

    pub fn put_evm_safe_observation(
        &mut self,
        value: &EvmSafeObservation,
    ) -> Result<(), StorageError> {
        let operation = self
            .evm_operation(value.operation_id.get())?
            .ok_or(StorageError::RecordNotFound)?;
        match operation.state {
            EvmOperationState::Submitted { transaction_hash }
                if transaction_hash == value.transaction_hash => {}
            _ => return Err(StorageError::Core(CoreError::ConflictingReplay)),
        }
        self.evm_safe_observations
            .insert(value.operation_id.get(), encode(value)?);
        Ok(())
    }

    pub fn remove_evm_safe_observation(&mut self, id: u64) {
        self.evm_safe_observations.remove(&id);
    }

    pub fn safe_evm_count(&self) -> u64 {
        self.evm_safe_observations.len()
    }

    pub fn first_submitted_for_safe_observation(
        &self,
        after_operation_id: u64,
    ) -> Result<Option<EvmOperationRecord>, StorageError> {
        let mut first = None;
        for entry in self.evm_operations.iter() {
            let operation: EvmOperationRecord = decode(&entry.value())?;
            if !matches!(operation.state, EvmOperationState::Submitted { .. }) {
                continue;
            }
            if first.is_none() {
                first = Some(operation);
            }
            if operation.id.get() > after_operation_id {
                return Ok(Some(operation));
            }
        }
        Ok(first)
    }

    fn put_reconciliation_hold(
        &mut self,
        value: &ReconciliationHoldRecord,
    ) -> Result<(), StorageError> {
        let encoded_value = encode(value)?;
        let previous = self
            .reconciliation_holds
            .get(&value.id.get())
            .map(|blob| decode::<ReconciliationHoldRecord>(&blob))
            .transpose()?;
        let mut counters = self.counters()?;
        counters.reconciliation_holds = adjust_active_count(
            counters.reconciliation_holds,
            previous.as_ref().map(is_open_hold).unwrap_or(false),
            is_open_hold(value),
        )?;
        let encoded_counters = encode(&counters)?;
        self.reconciliation_holds
            .insert(value.id.get(), encoded_value);
        self.counters.set(encoded_counters);
        Ok(())
    }

    pub fn put_open_reconciliation_hold(
        &mut self,
        value: &ReconciliationHoldRecord,
    ) -> Result<(), StorageError> {
        if !is_open_hold(value) {
            return Err(StorageError::Core(CoreError::HoldMismatch));
        }
        if let Some(previous) = self.reconciliation_hold(value.id.get())? {
            if previous == *value {
                return Ok(());
            }
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        self.put_reconciliation_hold(value)
    }

    pub fn reconciliation_hold(
        &self,
        id: u64,
    ) -> Result<Option<ReconciliationHoldRecord>, StorageError> {
        self.reconciliation_holds
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }

    pub fn first_open_hold(&self) -> Result<Option<ReconciliationHoldRecord>, StorageError> {
        for entry in self.reconciliation_holds.iter() {
            let hold: ReconciliationHoldRecord = decode(&entry.value())?;
            if matches!(hold.state, ReconciliationHoldState::Open) {
                return Ok(Some(hold));
            }
        }
        Ok(None)
    }

    pub fn resolve_deposit_hold(
        &mut self,
        deposit_id: DepositId,
        hold_id: HoldId,
        resolution: DepositHoldResolution,
    ) -> Result<ApplyResult, StorageError> {
        let mut deposit = self
            .deposit(deposit_id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        let mut hold = self
            .reconciliation_hold(hold_id.get())?
            .ok_or(StorageError::RecordNotFound)?;
        let result = resolve_deposit_hold(&mut deposit, &mut hold, resolution)?;
        self.persist_resolved_deposit_and_hold(&deposit, &hold)?;
        Ok(result)
    }

    pub fn resolve_withdrawal_hold(
        &mut self,
        withdrawal_id: WithdrawalId,
        hold_id: HoldId,
        resolution: WithdrawalHoldResolution,
    ) -> Result<ApplyResult, StorageError> {
        let mut withdrawal = self
            .withdrawal(withdrawal_id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        let mut hold = self
            .reconciliation_hold(hold_id.get())?
            .ok_or(StorageError::RecordNotFound)?;
        let result = resolve_withdrawal_hold(&mut withdrawal, &mut hold, resolution)?;
        self.persist_resolved_withdrawal_and_hold(&withdrawal, &hold)?;
        Ok(result)
    }

    fn persist_resolved_deposit_and_hold(
        &mut self,
        deposit: &DepositRecord,
        hold: &ReconciliationHoldRecord,
    ) -> Result<(), StorageError> {
        let encoded_deposit = encode(deposit)?;
        let encoded_hold = encode(hold)?;
        let previous_deposit = self.deposit(deposit.id.bytes())?;
        let mut counters = self.counters_after_hold_update(hold)?;
        counters.pending_ledger_operations = adjust_active_count(
            counters.pending_ledger_operations,
            previous_deposit
                .as_ref()
                .map(is_pending_deposit_ledger)
                .unwrap_or(false),
            is_pending_deposit_ledger(deposit),
        )?;
        counters.reserved_deposit_mint_amount = adjust_reserved_mint_amount(
            counters.reserved_deposit_mint_amount,
            previous_deposit.as_ref(),
            deposit,
        )?;
        let encoded_counters = encode(&counters)?;
        self.deposits.insert(deposit.id.bytes(), encoded_deposit);
        self.reconciliation_holds
            .insert(hold.id.get(), encoded_hold);
        self.counters.set(encoded_counters);
        Ok(())
    }

    fn persist_resolved_withdrawal_and_hold(
        &mut self,
        withdrawal: &WithdrawalRecord,
        hold: &ReconciliationHoldRecord,
    ) -> Result<(), StorageError> {
        let encoded_withdrawal = encode(withdrawal)?;
        let encoded_hold = encode(hold)?;
        let previous_withdrawal = self.withdrawal(withdrawal.id.bytes())?;
        let mut counters = self.counters_after_hold_update(hold)?;
        counters.pending_ledger_operations = adjust_active_count(
            counters.pending_ledger_operations,
            previous_withdrawal
                .as_ref()
                .map(is_pending_withdrawal_ledger)
                .unwrap_or(false),
            is_pending_withdrawal_ledger(withdrawal),
        )?;
        let encoded_counters = encode(&counters)?;
        self.withdrawals
            .insert(withdrawal.id.bytes(), encoded_withdrawal);
        self.reconciliation_holds
            .insert(hold.id.get(), encoded_hold);
        self.counters.set(encoded_counters);
        Ok(())
    }

    fn counters_after_hold_update(
        &self,
        hold: &ReconciliationHoldRecord,
    ) -> Result<CounterState, StorageError> {
        let previous = self
            .reconciliation_holds
            .get(&hold.id.get())
            .map(|blob| decode::<ReconciliationHoldRecord>(&blob))
            .transpose()?;
        let mut counters = self.counters()?;
        counters.reconciliation_holds = adjust_active_count(
            counters.reconciliation_holds,
            previous.as_ref().map(is_open_hold).unwrap_or(false),
            is_open_hold(hold),
        )?;
        Ok(counters)
    }

    pub fn status_counts(&self) -> Result<StorageCounts, StorageError> {
        let counters = self.counters()?;

        Ok(StorageCounts {
            deposits: self.deposits.len(),
            withdrawals: self.withdrawals.len(),
            pending_evm_operations: counters.pending_evm_operations,
            reconciliation_holds: counters.reconciliation_holds,
            pending_ledger_operations: counters.pending_ledger_operations,
            reserved_deposit_mint_amount: counters.reserved_deposit_mint_amount,
            reverted_evm_operations: counters.reverted_evm_operations,
            withdrawal_log_cursor: self.external_progress()?.withdrawal_log_cursor,
            last_finalized_base_block: self.external_progress()?.last_finalized_base_block,
        })
    }
}

fn is_pending_evm(value: &EvmOperationRecord) -> bool {
    !matches!(
        value.state,
        EvmOperationState::Finalized { .. } | EvmOperationState::Reverted { .. }
    )
}

fn is_reverted_evm(value: &EvmOperationRecord) -> bool {
    matches!(value.state, EvmOperationState::Reverted { .. })
}

fn is_open_hold(value: &ReconciliationHoldRecord) -> bool {
    matches!(value.state, ReconciliationHoldState::Open)
}

fn is_pending_deposit_ledger(value: &DepositRecord) -> bool {
    matches!(value.state, bridge_core::DepositState::PullPending)
}

fn is_deposit_mint_reserved(value: &DepositRecord) -> bool {
    !matches!(
        value.state,
        bridge_core::DepositState::Minted { .. } | bridge_core::DepositState::Cancelled { .. }
    )
}

fn adjust_reserved_mint_amount(
    current: u128,
    previous: Option<&DepositRecord>,
    next: &DepositRecord,
) -> Result<u128, StorageError> {
    let without_previous = if previous.is_some_and(is_deposit_mint_reserved) {
        current
            .checked_sub(previous.expect("checked previous").net_amount.get())
            .ok_or(StorageError::CounterUnderflow)?
    } else {
        current
    };
    if is_deposit_mint_reserved(next) {
        without_previous
            .checked_add(next.net_amount.get())
            .ok_or(StorageError::CounterOverflow)
    } else {
        Ok(without_previous)
    }
}

fn is_pending_withdrawal_ledger(value: &WithdrawalRecord) -> bool {
    matches!(value.state, WithdrawalState::ReleasePending { .. })
}

fn adjust_active_count(
    current: u64,
    was_active: bool,
    is_active: bool,
) -> Result<u64, StorageError> {
    match bridge_core::counter_delta(was_active, is_active) {
        1 => current.checked_add(1).ok_or(StorageError::CounterOverflow),
        -1 => current.checked_sub(1).ok_or(StorageError::CounterUnderflow),
        _ => Ok(current),
    }
}

fn encode<T: Serialize>(value: &T) -> Result<StableBlob, StorageError> {
    let mut bytes = vec![WIRE_VERSION];
    ciborium::into_writer(value, &mut bytes).map_err(|_| StorageError::EncodeFailed)?;
    StableBlob::new(bytes)
}

fn decode<T: DeserializeOwned>(blob: &StableBlob) -> Result<T, StorageError> {
    let (version, payload) = blob
        .as_slice()
        .split_first()
        .ok_or(StorageError::MissingWireVersion)?;
    if *version != WIRE_VERSION {
        return Err(StorageError::UnsupportedWireVersion(*version));
    }
    let mut cursor = Cursor::new(payload);
    let value = ciborium::from_reader(&mut cursor).map_err(|_| StorageError::DecodeFailed)?;
    if cursor.position() != payload.len() as u64 {
        return Err(StorageError::DecodeFailed);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::{
        Account, Amount, ApplyOutcome, BaseMintSnapshot, DepositEvent, DepositHoldResolution,
        DepositId, DepositRequest, DepositState, EvmCallIntent, EvmOperationEvent, EvmOperationId,
        EvmOperationKind, HoldId, LedgerOperation, LedgerTransferIdentity,
        ReconciliationHoldRecord, ReconciliationHoldState, RefundEligibility, RefundReason,
        RequestReference, SafeReceiptOutcome, Settlement, TransferAttempt, WithdrawalEvent,
        WithdrawalHoldResolution, WithdrawalId,
    };
    use ic_stable_structures::VectorMemory;

    fn account(tag: u8) -> Account {
        Account::new(vec![tag], [tag; 32]).expect("valid account")
    }

    fn transfer(operation: LedgerOperation, amount: u128, tag: u8) -> LedgerTransferIdentity {
        LedgerTransferIdentity {
            operation,
            created_at_time_ns: u64::from(tag),
            memo: [tag; 32],
            amount: Amount::new(amount),
            fee: Amount::new(1),
            from: account(tag),
            to: account(tag + 1),
            spender: None,
        }
    }

    fn deposit() -> DepositRecord {
        let mut deposit = DepositRecord::accept(
            DepositRequest {
                id: DepositId::new([1; 32]),
                payload_hash: [2; 32],
                gross_amount: Amount::new(110),
                user_max_service_fee: Amount::new(10),
                transfer: transfer(LedgerOperation::PullDeposit, 110, 10),
            },
            BaseMintSnapshot {
                finalized_block_number: 1,
                finalized_block_timestamp: 1,
                service_fee: Amount::new(10),
                max_service_fee: Amount::new(20),
                per_deposit_limit: Amount::new(1_000),
                mint_window_limit: Amount::new(10_000),
                mint_window_started_at: 0,
                mint_window_duration: 100,
                minted_in_window: Amount::ZERO,
            },
        )
        .expect("valid deposit");
        deposit
            .apply(DepositEvent::PullSucceeded {
                ledger_block_index: 4,
            })
            .expect("escrowed");
        deposit
    }

    fn withdrawal() -> WithdrawalRecord {
        let mut withdrawal = WithdrawalRecord::observed(
            WithdrawalId::new([3; 32]),
            [4; 32],
            Amount::new(100),
            Amount::new(80),
            Amount::new(10),
        )
        .expect("valid withdrawal");
        withdrawal
            .apply(WithdrawalEvent::StartRelease {
                attempt: Box::new(TransferAttempt {
                    attempt_no: 0,
                    identity: transfer(LedgerOperation::ReleaseWithdrawal, 85, 20),
                }),
                settlement: Settlement {
                    amount_out: Amount::new(85),
                    service_fee: Amount::new(10),
                    ledger_fee: Amount::new(5),
                },
            })
            .expect("release pending");
        withdrawal
    }

    fn config() -> BridgeInitArgs {
        let principal = Principal::self_authenticating([7; 32]);
        BridgeInitArgs {
            ledger_canister_id: principal,
            index_canister_id: principal,
            evm_rpc_canister_id: principal,
            custom_evm_rpc_urls: vec![],
            base_chain_id: 8453,
            bridge_contract: vec![1; 20],
            ecdsa_key_name: "test_key".into(),
            ecdsa_derivation_path: vec![],
            poll_interval_seconds: 60,
            transaction_gas_limit: 500_000,
            max_fee_per_gas: 10,
            max_priority_fee_per_gas: 1,
            eth_floor_wei: 1,
            cycles_floor: 1,
            settlement_cycle_ceiling: 1,
            governance_principal: principal,
            pause_principals: vec![principal],
            finance_administrator: principal,
            fee_recipient: FeeRecipientConfig {
                owner: principal,
                subaccount: vec![],
            },
        }
    }

    fn held_deposit() -> (DepositRecord, ReconciliationHoldRecord) {
        let mut deposit = DepositRecord::accept(
            DepositRequest {
                id: DepositId::new([11; 32]),
                payload_hash: [12; 32],
                gross_amount: Amount::new(110),
                user_max_service_fee: Amount::new(10),
                transfer: transfer(LedgerOperation::PullDeposit, 110, 40),
            },
            BaseMintSnapshot {
                finalized_block_number: 1,
                finalized_block_timestamp: 1,
                service_fee: Amount::new(10),
                max_service_fee: Amount::new(20),
                per_deposit_limit: Amount::new(1_000),
                mint_window_limit: Amount::new(10_000),
                mint_window_started_at: 0,
                mint_window_duration: 100,
                minted_in_window: Amount::ZERO,
            },
        )
        .expect("valid deposit");
        let hold_id = HoldId::new(12);
        deposit
            .apply(DepositEvent::PullAmbiguous { hold_id })
            .expect("hold deposit");
        let hold = ReconciliationHoldRecord::open(
            hold_id,
            RequestReference::Deposit(deposit.id),
            deposit.transfer.clone(),
        );
        (deposit, hold)
    }

    #[test]
    fn every_incomplete_record_survives_reopen() {
        let memory = VectorMemory::default();
        let deposit = deposit();
        let withdrawal = withdrawal();
        let mut evm = EvmOperationRecord::prepared(
            EvmOperationId::new(6),
            [6; 32],
            EvmOperationKind::AcknowledgeRelease,
        );
        evm.apply(EvmOperationEvent::Submitted {
            transaction_hash: [8; 32],
        })
        .expect("submit evm");
        let safe = EvmSafeObservation {
            operation_id: evm.id,
            transaction_hash: [8; 32],
            receipt_block_number: 99,
            safe_block_number: 100,
            observed_at_ns: 101,
            outcome: SafeReceiptOutcome::Succeeded,
        };
        let hold = ReconciliationHoldRecord::open(
            HoldId::new(7),
            RequestReference::Withdrawal(withdrawal.id),
            transfer(LedgerOperation::ReleaseWithdrawal, 85, 30),
        );
        let accounting = AccountingState {
            fee_reserve: Amount::new(11),
            confirmed_deposit_fees: Amount::new(5),
            confirmed_withdrawal_fees: Amount::new(6),
        };
        let counters = CounterState {
            next_evm_operation_id: 8,
            next_hold_id: 9,
            pending_evm_operations: 1,
            reconciliation_holds: 1,
            pending_ledger_operations: 1,
            reserved_deposit_mint_amount: 100,
            ..CounterState::default()
        };

        {
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            store.put_deposit(&deposit).expect("write deposit");
            store.put_withdrawal(&withdrawal).expect("write withdrawal");
            store.put_evm_operation(&evm).expect("write evm");
            store
                .put_evm_safe_observation(&safe)
                .expect("write safe observation");
            store.put_reconciliation_hold(&hold).expect("write hold");
            store.set_accounting(&accounting).expect("write accounting");
            store.set_counters(&counters).expect("write counters");
        }

        let reopened = StableStore::init(memory).expect("reopen");
        assert_eq!(reopened.schema_version(), SCHEMA_VERSION);
        assert_eq!(
            reopened.deposit(deposit.id.bytes()).expect("read deposit"),
            Some(deposit)
        );
        assert_eq!(
            reopened
                .withdrawal(withdrawal.id.bytes())
                .expect("read withdrawal"),
            Some(withdrawal)
        );
        assert_eq!(
            reopened.evm_operation(evm.id.get()).expect("read evm"),
            Some(evm)
        );
        assert_eq!(
            reopened
                .evm_safe_observation(safe.operation_id.get())
                .expect("read safe observation"),
            Some(safe)
        );
        assert_eq!(reopened.safe_evm_count(), 1);
        assert_eq!(
            reopened
                .reconciliation_hold(hold.id.get())
                .expect("read hold"),
            Some(hold)
        );
        assert_eq!(reopened.accounting().expect("read accounting"), accounting);
        assert_eq!(reopened.counters().expect("read counters"), counters);
        assert_eq!(
            reopened.status_counts().expect("counts"),
            StorageCounts {
                deposits: 1,
                withdrawals: 1,
                pending_evm_operations: 1,
                reconciliation_holds: 1,
                pending_ledger_operations: 1,
                reserved_deposit_mint_amount: 100,
                reverted_evm_operations: 0,
                withdrawal_log_cursor: 0,
                last_finalized_base_block: 0,
            }
        );
    }

    #[test]
    fn schema_wire_corruption_and_size_are_rejected() {
        assert_eq!(
            StableBlob::new(vec![0; MAX_STABLE_VALUE_BYTES + 1]),
            Err(StorageError::ValueTooLarge {
                actual: MAX_STABLE_VALUE_BYTES + 1,
                maximum: MAX_STABLE_VALUE_BYTES,
            })
        );
        assert_eq!(
            decode::<CounterState>(&StableBlob::new(vec![2, 0]).expect("bounded")),
            Err(StorageError::UnsupportedWireVersion(2))
        );
        assert_eq!(
            decode::<CounterState>(&StableBlob::new(vec![1, 0xff]).expect("bounded")),
            Err(StorageError::DecodeFailed)
        );

        let memory = VectorMemory::default();
        let manager = MemoryManager::init(memory.clone());
        StableCell::init(manager.get(SCHEMA_MEMORY_ID), 99u16);
        assert!(matches!(
            StableStore::init(memory),
            Err(StorageError::UnsupportedSchemaVersion(99))
        ));
    }

    #[test]
    fn schema_v4_is_rejected_without_legacy_migration() {
        let memory = VectorMemory::default();
        let manager = MemoryManager::init(memory.clone());
        StableCell::init(manager.get(SCHEMA_MEMORY_ID), 4u16);
        assert!(matches!(
            StableStore::init(memory),
            Err(StorageError::UnsupportedSchemaVersion(4))
        ));
    }

    #[test]
    fn active_counters_follow_insert_replay_and_terminal_updates() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let mut evm = EvmOperationRecord::prepared(
            EvmOperationId::new(1),
            [1; 32],
            EvmOperationKind::MintDeposit,
        );
        store.put_evm_operation(&evm).expect("insert pending EVM");
        store.put_evm_operation(&evm).expect("replay pending EVM");
        assert_eq!(
            store.counters().expect("counters").pending_evm_operations,
            1
        );
        evm.state = EvmOperationState::Finalized {
            transaction_hash: [2; 32],
            finalized_block_number: 3,
        };
        store.put_evm_operation(&evm).expect("finalize EVM");
        store.put_evm_operation(&evm).expect("replay finalized EVM");
        assert_eq!(
            store.counters().expect("counters").pending_evm_operations,
            0
        );

        let (_, mut hold) = held_deposit();
        store
            .put_open_reconciliation_hold(&hold)
            .expect("insert open hold");
        store
            .put_open_reconciliation_hold(&hold)
            .expect("replay open hold");
        assert_eq!(store.counters().expect("counters").reconciliation_holds, 1);
        hold.state = ReconciliationHoldState::ResolvedAbsent {
            history_watermark: 9,
        };
        store
            .put_reconciliation_hold(&hold)
            .expect("resolve hold internally");
        store
            .put_reconciliation_hold(&hold)
            .expect("replay resolved hold internally");
        assert_eq!(store.counters().expect("counters").reconciliation_holds, 0);
    }

    #[test]
    fn mint_reservation_is_released_only_by_minted_or_cancelled() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let mut pending = deposit();
        store
            .put_deposit(&pending)
            .expect("reserve escrowed deposit");
        store.put_deposit(&pending).expect("replay reservation");
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .reserved_deposit_mint_amount,
            100
        );
        let operation_id = EvmOperationId::new(41);
        pending
            .apply(DepositEvent::PrepareMint { operation_id })
            .expect("prepare mint");
        store
            .put_deposit(&pending)
            .expect("retain pending reservation");
        pending
            .apply(DepositEvent::MintReverted { operation_id })
            .expect("terminal revert");
        store
            .put_deposit(&pending)
            .expect("retain reverted reservation");
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .reserved_deposit_mint_amount,
            100
        );

        let mut minted = deposit();
        minted
            .apply(DepositEvent::PrepareMint { operation_id })
            .expect("prepare replacement fixture");
        minted
            .apply(DepositEvent::MintFinalized { operation_id })
            .expect("finalize mint");
        store
            .put_deposit(&minted)
            .expect("release minted reservation");
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .reserved_deposit_mint_amount,
            0
        );
    }

    #[test]
    fn definitive_pull_failure_releases_reservation_and_pending_counter() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let mut pending = deposit();
        pending.state = DepositState::PullPending;
        store
            .put_deposit(&pending)
            .expect("reserve pending deposit");
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .reserved_deposit_mint_amount,
            100
        );
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .pending_ledger_operations,
            1
        );
        pending
            .apply(DepositEvent::PullFailed {
                code: bridge_core::LedgerFailure::BadFee {
                    expected_fee: Amount::new(2),
                },
            })
            .expect("cancel deposit");
        store.put_deposit(&pending).expect("persist cancellation");
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .reserved_deposit_mint_amount,
            0
        );
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .pending_ledger_operations,
            0
        );
    }

    #[test]
    fn reverted_evm_counter_is_constant_time_and_idempotent() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let mut evm = EvmOperationRecord::prepared(
            EvmOperationId::new(42),
            [4; 32],
            EvmOperationKind::MintDeposit,
        );
        store.put_evm_operation(&evm).expect("insert pending");
        evm.state = EvmOperationState::Reverted {
            transaction_hash: [5; 32],
            finalized_block_number: 99,
        };
        store.put_evm_operation(&evm).expect("mark reverted");
        store.put_evm_operation(&evm).expect("replay reverted");
        let counts = store.status_counts().expect("status");
        assert_eq!(counts.pending_evm_operations, 0);
        assert_eq!(counts.reverted_evm_operations, 1);
    }

    #[test]
    fn acknowledgement_bundle_is_all_or_error_on_operation_id_overflow() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let mut withdrawal = withdrawal();
        withdrawal
            .apply(WithdrawalEvent::ReleaseSucceeded {
                ledger_block_index: 77,
            })
            .expect("release transferred");
        store
            .put_withdrawal(&withdrawal)
            .expect("persist release transfer");
        store
            .set_counters(&CounterState {
                next_evm_operation_id: u64::MAX,
                ..store.counters().expect("counters")
            })
            .expect("seed operation id overflow");
        let before = withdrawal.clone();
        assert_eq!(
            crate::tasks::prepare_acknowledgement_in_store(&mut store, &config(), &mut withdrawal,),
            Err(StorageError::CounterOverflow)
        );
        assert_eq!(withdrawal, before);
        assert_eq!(
            store
                .withdrawal(withdrawal.id.bytes())
                .expect("read withdrawal"),
            Some(before)
        );
        assert_eq!(store.queued_evm_count().expect("queued count"), 0);
    }

    #[test]
    fn acknowledgement_bundle_persists_parent_operation_and_intent_together() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let mut withdrawal = withdrawal();
        withdrawal
            .apply(WithdrawalEvent::ReleaseSucceeded {
                ledger_block_index: 78,
            })
            .expect("release transferred");
        store
            .put_withdrawal(&withdrawal)
            .expect("persist release transfer");
        crate::tasks::prepare_acknowledgement_in_store(&mut store, &config(), &mut withdrawal)
            .expect("prepare acknowledgement");
        let operation_id = match &withdrawal.state {
            WithdrawalState::AcknowledgePending { operation_id, .. } => *operation_id,
            _ => panic!("withdrawal must be acknowledgement pending"),
        };
        assert!(store
            .evm_operation(operation_id.get())
            .expect("operation")
            .is_some());
        assert!(store
            .evm_call_intent(operation_id.get())
            .expect("intent")
            .is_some());
        assert_eq!(
            store
                .withdrawal(withdrawal.id.bytes())
                .expect("stored withdrawal"),
            Some(withdrawal)
        );
    }

    #[test]
    fn refund_bundle_is_inserted_only_once_for_a_withdrawal() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let operation_id = EvmOperationId::new(9);
        let mut withdrawal = WithdrawalRecord::observed(
            WithdrawalId::new([8; 32]),
            [7; 32],
            Amount::new(2),
            Amount::new(2),
            Amount::new(10),
        )
        .expect("observed withdrawal");
        withdrawal
            .apply(WithdrawalEvent::StartRefund {
                operation_id,
                eligibility: RefundEligibility {
                    finalized_base_block: 100,
                    base_status_pending: true,
                    release_attempt_created: false,
                    reason: RefundReason::AmountBelowMinimum,
                },
            })
            .expect("prepare refund");
        let operation = EvmOperationRecord::queued(
            operation_id,
            withdrawal.payload_hash,
            EvmOperationKind::RefundWithdrawal,
        );
        let intent = EvmCallIntent {
            operation_id,
            payload_hash: withdrawal.payload_hash,
            chain_id: 8453,
            contract: [1; 20],
            calldata: vec![1, 2, 3, 4],
            gas_limit: 100_000,
            max_fee_per_gas: 10,
            max_priority_fee_per_gas: 1,
        };

        assert!(store
            .put_refund_if_absent(&withdrawal, &operation, &intent)
            .expect("first insert"));
        assert!(!store
            .put_refund_if_absent(&withdrawal, &operation, &intent)
            .expect("idempotent replay"));
        assert_eq!(store.queued_evm_count().expect("queued count"), 1);
        assert_eq!(store.withdrawals.len(), 1);
    }

    #[test]
    fn resolved_deposit_and_hold_survive_reopen_and_retry_together() {
        let memory = VectorMemory::default();
        let (deposit, hold) = held_deposit();
        {
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            store.put_deposit(&deposit).expect("write deposit");
            store
                .put_open_reconciliation_hold(&hold)
                .expect("write hold");
            assert_eq!(
                store
                    .resolve_deposit_hold(
                        deposit.id,
                        hold.id,
                        DepositHoldResolution::Succeeded {
                            ledger_block_index: 88,
                        },
                    )
                    .expect("resolve")
                    .outcome,
                ApplyOutcome::Applied
            );
            assert_eq!(store.counters().expect("counters").reconciliation_holds, 0);
        }

        let mut reopened = StableStore::init(memory).expect("reopen");
        assert!(matches!(
            reopened
                .deposit(deposit.id.bytes())
                .expect("read deposit")
                .expect("deposit exists")
                .state,
            DepositState::Escrowed {
                ledger_block_index: 88
            }
        ));
        assert_eq!(
            reopened
                .reconciliation_hold(hold.id.get())
                .expect("read hold")
                .expect("hold exists")
                .state,
            ReconciliationHoldState::ResolvedSucceeded {
                ledger_block_index: 88
            }
        );
        assert_eq!(
            reopened
                .resolve_deposit_hold(
                    deposit.id,
                    hold.id,
                    DepositHoldResolution::Succeeded {
                        ledger_block_index: 88,
                    },
                )
                .expect("retry resolution")
                .outcome,
            ApplyOutcome::Idempotent
        );
        assert_eq!(
            reopened.counters().expect("counters").reconciliation_holds,
            0
        );
    }

    #[test]
    fn withdrawal_and_hold_absence_resolution_is_persisted_together() {
        let memory = VectorMemory::default();
        let mut withdrawal = withdrawal();
        let hold_id = HoldId::new(50);
        withdrawal
            .apply(WithdrawalEvent::ReleaseAmbiguous { hold_id })
            .expect("hold withdrawal");
        let hold_transfer = match &withdrawal.state {
            bridge_core::WithdrawalState::ReconciliationHold { attempt, .. } => {
                attempt.identity.clone()
            }
            _ => panic!("withdrawal must be held"),
        };
        let hold = ReconciliationHoldRecord::open(
            hold_id,
            RequestReference::Withdrawal(withdrawal.id),
            hold_transfer,
        );
        let mut store = StableStore::init(memory).expect("initialize");
        store.put_withdrawal(&withdrawal).expect("write withdrawal");
        store
            .put_open_reconciliation_hold(&hold)
            .expect("write hold");
        let mut next_identity = hold.transfer.clone();
        next_identity.created_at_time_ns += 1;
        next_identity.memo = [99; 32];
        let resolution = WithdrawalHoldResolution::Absent {
            history_watermark: 500,
            next_identity: Box::new(next_identity),
        };
        assert_eq!(
            store
                .resolve_withdrawal_hold(withdrawal.id, hold.id, resolution.clone())
                .expect("resolve absent")
                .outcome,
            ApplyOutcome::Applied
        );
        assert_eq!(
            store
                .resolve_withdrawal_hold(withdrawal.id, hold.id, resolution)
                .expect("retry absent")
                .outcome,
            ApplyOutcome::Idempotent
        );
        assert_eq!(store.counters().expect("counters").reconciliation_holds, 0);
    }

    #[test]
    fn status_counts_do_not_decode_historical_records() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        for id in 0..100 {
            let mut evm = EvmOperationRecord::prepared(
                EvmOperationId::new(id),
                [1; 32],
                EvmOperationKind::MintDeposit,
            );
            evm.state = EvmOperationState::Finalized {
                transaction_hash: [2; 32],
                finalized_block_number: id,
            };
            store.put_evm_operation(&evm).expect("write terminal EVM");
        }
        store.evm_operations.insert(
            200,
            StableBlob::new(vec![WIRE_VERSION, 0xff]).expect("bounded corruption"),
        );
        assert_eq!(
            store.status_counts().expect("constant-time counts"),
            StorageCounts {
                deposits: 0,
                withdrawals: 0,
                pending_evm_operations: 0,
                reconciliation_holds: 0,
                pending_ledger_operations: 0,
                reserved_deposit_mint_amount: 0,
                reverted_evm_operations: 0,
                withdrawal_log_cursor: 0,
                last_finalized_base_block: 0,
            }
        );
    }

    #[test]
    fn counter_overflow_and_underflow_fail_before_record_write() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        store
            .set_counters(&CounterState {
                pending_evm_operations: u64::MAX,
                ..CounterState::default()
            })
            .expect("seed overflow counter");
        let evm = EvmOperationRecord::prepared(
            EvmOperationId::new(1),
            [1; 32],
            EvmOperationKind::MintDeposit,
        );
        assert_eq!(
            store.put_evm_operation(&evm),
            Err(StorageError::CounterOverflow)
        );
        assert_eq!(store.evm_operation(evm.id.get()).expect("read EVM"), None);

        store
            .set_counters(&CounterState::default())
            .expect("reset counters");
        store.put_evm_operation(&evm).expect("insert pending EVM");
        store
            .set_counters(&CounterState::default())
            .expect("corrupt count for underflow fixture");
        let mut finalized = evm;
        finalized.state = EvmOperationState::Finalized {
            transaction_hash: [2; 32],
            finalized_block_number: 3,
        };
        assert_eq!(
            store.put_evm_operation(&finalized),
            Err(StorageError::CounterUnderflow)
        );
        assert_eq!(
            store.evm_operation(evm.id.get()).expect("read EVM"),
            Some(evm)
        );
    }

    #[test]
    fn reserved_memory_ids_are_never_reassigned() {
        assert_eq!(RESERVED_MEMORY_IDS, 17..=31);
    }
}
