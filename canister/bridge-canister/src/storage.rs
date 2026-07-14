use crate::config::BridgeInitArgs;
use crate::{admin::AdminState, config::FeeRecipientConfig};
use bridge_core::{
    resolve_deposit_hold, resolve_withdrawal_hold, AccountingState, ApplyResult, BaseMintSnapshot,
    CoreError, DepositHoldResolution, DepositId, DepositRecord, EvmCallIntent, EvmOperationRecord,
    EvmOperationState, EvmTransactionEnvelope, ExternalProgress, HoldId, ReconciliationHoldRecord,
    ReconciliationHoldState, ReconciliationScanProgress, WithdrawalHoldResolution, WithdrawalId,
    WithdrawalRecord, WithdrawalState,
};
use candid::{CandidType, Principal};
use ic_stable_structures::{
    memory_manager::{MemoryId, MemoryManager, VirtualMemory},
    storable::{Bound, Storable},
    Memory, StableBTreeMap, StableCell,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{borrow::Cow, fmt, io::Cursor};

pub const SCHEMA_VERSION: u16 = 1;
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
const WITHDRAWAL_NOTIFICATIONS_MEMORY_ID: MemoryId = MemoryId::new(16);
const DEPOSIT_OWNER_INDEX_MEMORY_ID: MemoryId = MemoryId::new(17);
const DEPOSIT_ADMISSION_MEMORY_ID: MemoryId = MemoryId::new(18);
const FEE_PAYOUT_STATE_INDEX_MEMORY_ID: MemoryId = MemoryId::new(19);
const OPERATION_OWNER_INDEX_MEMORY_ID: MemoryId = MemoryId::new(20);
const EVM_STATE_INDEX_MEMORY_ID: MemoryId = MemoryId::new(21);
const PULL_PENDING_DEPOSIT_INDEX_MEMORY_ID: MemoryId = MemoryId::new(22);
const RELEASE_PENDING_WITHDRAWAL_INDEX_MEMORY_ID: MemoryId = MemoryId::new(23);
const OPEN_HOLD_INDEX_MEMORY_ID: MemoryId = MemoryId::new(24);
const WITHDRAWAL_NOTIFICATION_CONTROL_MEMORY_ID: MemoryId = MemoryId::new(25);
pub const RESERVED_MEMORY_IDS: core::ops::RangeInclusive<u8> = 26..=31;

type StableMemory<M> = VirtualMemory<M>;

fn deposit_owner_index_prefix(owner: Principal) -> Vec<u8> {
    let owner_bytes = owner.as_slice();
    let mut prefix = Vec::with_capacity(1 + owner_bytes.len());
    prefix.push(owner_bytes.len() as u8);
    prefix.extend_from_slice(owner_bytes);
    prefix
}

fn deposit_owner_index_bytes(prefix: &[u8], reverse_sequence: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(prefix.len() + 8);
    bytes.extend_from_slice(prefix);
    bytes.extend_from_slice(&reverse_sequence.to_be_bytes());
    bytes
}

fn deposit_owner_index_key(owner: Principal, sequence: u64) -> Result<StableBlob, StorageError> {
    StableBlob::new(deposit_owner_index_bytes(
        &deposit_owner_index_prefix(owner),
        u64::MAX - sequence,
    ))
}

fn deposit_sequence_from_index_key(key: &StableBlob) -> Result<u64, StorageError> {
    let reverse_bytes: [u8; 8] = key
        .as_slice()
        .get(key.as_slice().len().saturating_sub(8)..)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(StorageError::DecodeFailed)?;
    Ok(u64::MAX - u64::from_be_bytes(reverse_bytes))
}

fn fee_payout_index_key_for_state(id: u64, state: u8) -> Result<StableBlob, StorageError> {
    let mut bytes = Vec::with_capacity(9);
    bytes.push(state);
    bytes.extend_from_slice(&id.to_be_bytes());
    StableBlob::new(bytes)
}

fn fee_payout_index_key(
    value: &crate::admin::FeePayoutRecord,
) -> Result<Option<StableBlob>, StorageError> {
    matches!(
        value.state,
        crate::admin::FeePayoutState::Pending | crate::admin::FeePayoutState::ReconciliationHold
    )
    .then(|| fee_payout_index_key_for_state(value.id, 0))
    .transpose()
}

fn fee_payout_id_from_index_key(key: &StableBlob) -> Result<u64, StorageError> {
    let bytes: [u8; 8] = key
        .as_slice()
        .get(1..9)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(StorageError::DecodeFailed)?;
    Ok(u64::from_be_bytes(bytes))
}

fn evm_state_tag(state: EvmOperationState) -> Option<u8> {
    match state {
        EvmOperationState::Queued => Some(0),
        EvmOperationState::Prepared => Some(1),
        EvmOperationState::Submitted { .. } => Some(2),
        EvmOperationState::Finalized { .. } | EvmOperationState::Reverted { .. } => None,
    }
}

fn evm_state_index_key(value: &EvmOperationRecord) -> Result<Option<StableBlob>, StorageError> {
    let Some(tag) = evm_state_tag(value.state) else {
        return Ok(None);
    };
    let priority = matches!(value.state, EvmOperationState::Queued)
        .then(|| value.kind.scheduler_priority())
        .unwrap_or(0);
    let mut bytes = Vec::with_capacity(10);
    bytes.push(tag);
    bytes.push(priority);
    bytes.extend_from_slice(&value.id.get().to_be_bytes());
    StableBlob::new(bytes).map(Some)
}

fn first_evm_index_id(
    index: &StableBTreeMap<StableBlob, u8, impl Memory>,
    tag: u8,
) -> Result<Option<u64>, StorageError> {
    let start = StableBlob::new(vec![tag])?;
    let end = StableBlob::new(vec![tag.saturating_add(1)])?;
    let Some(entry) = index.range(start..end).next() else {
        return Ok(None);
    };
    evm_index_id(entry.key()).map(Some)
}

fn evm_index_id(key: &StableBlob) -> Result<u64, StorageError> {
    let bytes: [u8; 8] = key
        .as_slice()
        .get(2..10)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(StorageError::DecodeFailed)?;
    Ok(u64::from_be_bytes(bytes))
}

fn deposit_operation_id(value: &DepositRecord) -> Option<u64> {
    match value.state {
        bridge_core::DepositState::MintPending { operation_id, .. }
        | bridge_core::DepositState::Minted { operation_id, .. }
        | bridge_core::DepositState::MintReverted { operation_id, .. } => Some(operation_id.get()),
        _ => None,
    }
}

fn withdrawal_operation_id(value: &WithdrawalRecord) -> Option<u64> {
    match value.state {
        WithdrawalState::AcknowledgePending { operation_id, .. }
        | WithdrawalState::AcknowledgeReverted { operation_id, .. }
        | WithdrawalState::Released { operation_id, .. }
        | WithdrawalState::RefundPending { operation_id, .. }
        | WithdrawalState::RefundReverted { operation_id, .. }
        | WithdrawalState::Refunded { operation_id } => Some(operation_id.get()),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
    pub pending_ledger_operations: u64,
    pub next_audit_sequence: u64,
    pub next_fee_payout_id: u64,
    pub next_deposit_sequence: u64,
    pub reserved_deposit_mint_amount: u128,
    pub reverted_evm_operations: u64,
    pub queued_evm_operations: u64,
    pub nonterminal_withdrawals: u64,
    pub pending_fee_payout_debit: u128,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepositCallerQuota {
    pub caller: Vec<u8>,
    pub count: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedBaseMintSnapshot {
    pub observed_at_ns: u64,
    pub snapshot: BaseMintSnapshot,
    pub bridge_signer: [u8; 20],
    pub deposits_paused: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepositAdmissionControl {
    pub window_id: u64,
    pub global_count: u16,
    pub caller_counts: Vec<DepositCallerQuota>,
    pub signer_address: Option<[u8; 20]>,
    pub signer_public_key: Option<Vec<u8>>,
    pub base_snapshot: Option<CachedBaseMintSnapshot>,
    pub refresh_started_at_ns: Option<u64>,
    pub next_refresh_allowed_at_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepositRateLimit {
    pub retry_after_seconds: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WithdrawalNotification {
    pub transaction_hash: [u8; 32],
    pub caller: Principal,
    pub created_at_ns: u64,
    pub next_attempt_at_ns: u64,
    pub attempts: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NotificationCallerQuota {
    caller: Principal,
    count: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct WithdrawalNotificationControl {
    window_id: u64,
    global_count: u8,
    caller_counts: Vec<NotificationCallerQuota>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationEnqueueOutcome {
    Queued,
    Duplicate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationEnqueueError {
    RateLimited { retry_after_seconds: u64 },
    QueueFull,
    Storage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DepositQuotaError {
    RateLimited(DepositRateLimit),
    Storage(StorageError),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum OperationOwner {
    Deposit([u8; 32]),
    Withdrawal([u8; 32]),
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
    pub last_finalized_base_block: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepositIntent {
    pub deposit_id: [u8; 32],
    pub caller: Vec<u8>,
    pub client_request_id: [u8; 32],
    pub base_recipient: [u8; 20],
    pub from_subaccount: [u8; 32],
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
    withdrawal_notifications: StableBTreeMap<[u8; 32], StableBlob, StableMemory<M>>,
    deposit_owner_index: StableBTreeMap<StableBlob, [u8; 32], StableMemory<M>>,
    deposit_admission: StableCell<StableBlob, StableMemory<M>>,
    fee_payout_state_index: StableBTreeMap<StableBlob, u8, StableMemory<M>>,
    operation_owner_index: StableBTreeMap<u64, StableBlob, StableMemory<M>>,
    evm_state_index: StableBTreeMap<StableBlob, u8, StableMemory<M>>,
    pull_pending_deposit_index: StableBTreeMap<[u8; 32], u8, StableMemory<M>>,
    release_pending_withdrawal_index: StableBTreeMap<[u8; 32], u8, StableMemory<M>>,
    open_hold_index: StableBTreeMap<u64, u8, StableMemory<M>>,
    withdrawal_notification_control: StableCell<StableBlob, StableMemory<M>>,
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
            withdrawal_notifications: StableBTreeMap::init(
                manager.get(WITHDRAWAL_NOTIFICATIONS_MEMORY_ID),
            ),
            deposit_owner_index: StableBTreeMap::init(manager.get(DEPOSIT_OWNER_INDEX_MEMORY_ID)),
            deposit_admission: StableCell::init(
                manager.get(DEPOSIT_ADMISSION_MEMORY_ID),
                encode(&DepositAdmissionControl::default())?,
            ),
            fee_payout_state_index: StableBTreeMap::init(
                manager.get(FEE_PAYOUT_STATE_INDEX_MEMORY_ID),
            ),
            operation_owner_index: StableBTreeMap::init(
                manager.get(OPERATION_OWNER_INDEX_MEMORY_ID),
            ),
            evm_state_index: StableBTreeMap::init(manager.get(EVM_STATE_INDEX_MEMORY_ID)),
            pull_pending_deposit_index: StableBTreeMap::init(
                manager.get(PULL_PENDING_DEPOSIT_INDEX_MEMORY_ID),
            ),
            release_pending_withdrawal_index: StableBTreeMap::init(
                manager.get(RELEASE_PENDING_WITHDRAWAL_INDEX_MEMORY_ID),
            ),
            open_hold_index: StableBTreeMap::init(manager.get(OPEN_HOLD_INDEX_MEMORY_ID)),
            withdrawal_notification_control: StableCell::init(
                manager.get(WITHDRAWAL_NOTIFICATION_CONTROL_MEMORY_ID),
                encode(&WithdrawalNotificationControl::default())?,
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

    fn deposit_admission(&self) -> Result<DepositAdmissionControl, StorageError> {
        decode(self.deposit_admission.get())
    }

    fn set_deposit_admission(
        &mut self,
        value: &DepositAdmissionControl,
    ) -> Result<(), StorageError> {
        self.deposit_admission.set(encode(value)?);
        Ok(())
    }

    pub fn reserve_deposit_quota(
        &mut self,
        caller: Principal,
        now_ns: u64,
        window_seconds: u64,
        global_limit: u16,
        per_principal_limit: u16,
    ) -> Result<(), DepositQuotaError> {
        let window_ns = window_seconds.saturating_mul(1_000_000_000);
        let window_id = now_ns / window_ns;
        let mut admission = self
            .deposit_admission()
            .map_err(DepositQuotaError::Storage)?;
        if admission.window_id != window_id {
            admission.window_id = window_id;
            admission.global_count = 0;
            admission.caller_counts.clear();
        }
        let retry_after_seconds = ((window_id + 1)
            .saturating_mul(window_ns)
            .saturating_sub(now_ns)
            .saturating_add(999_999_999)
            / 1_000_000_000)
            .max(1);
        let caller_bytes = caller.as_slice();
        let caller_count = admission
            .caller_counts
            .iter()
            .find(|entry| entry.caller == caller_bytes)
            .map(|entry| entry.count)
            .unwrap_or(0);
        if admission.global_count >= global_limit || caller_count >= per_principal_limit {
            return Err(DepositQuotaError::RateLimited(DepositRateLimit {
                retry_after_seconds,
            }));
        }
        admission.global_count = admission.global_count.saturating_add(1);
        match admission
            .caller_counts
            .iter_mut()
            .find(|entry| entry.caller == caller_bytes)
        {
            Some(entry) => entry.count = entry.count.saturating_add(1),
            None => admission.caller_counts.push(DepositCallerQuota {
                caller: caller_bytes.to_vec(),
                count: 1,
            }),
        }
        self.set_deposit_admission(&admission)
            .map_err(DepositQuotaError::Storage)
    }

    pub fn cached_base_mint_snapshot(
        &self,
        now_ns: u64,
        ttl_ns: u64,
        minimum_finalized_block: u64,
    ) -> Result<Option<CachedBaseMintSnapshot>, StorageError> {
        Ok(self.deposit_admission()?.base_snapshot.and_then(|cached| {
            (now_ns.saturating_sub(cached.observed_at_ns) <= ttl_ns
                && cached.snapshot.finalized_block_number >= minimum_finalized_block)
                .then_some(cached)
        }))
    }

    pub fn begin_base_snapshot_refresh(
        &mut self,
        now_ns: u64,
        stale_lock_ns: u64,
        cooldown_ns: u64,
    ) -> Result<bool, StorageError> {
        let mut admission = self.deposit_admission()?;
        let locked = admission
            .refresh_started_at_ns
            .is_some_and(|started| now_ns.saturating_sub(started) < stale_lock_ns);
        if locked || now_ns < admission.next_refresh_allowed_at_ns {
            return Ok(false);
        }
        admission.refresh_started_at_ns = Some(now_ns);
        admission.next_refresh_allowed_at_ns = now_ns.saturating_add(cooldown_ns);
        self.set_deposit_admission(&admission)?;
        Ok(true)
    }

    pub fn finish_base_snapshot_refresh(
        &mut self,
        observed_at_ns: u64,
        snapshot: BaseMintSnapshot,
        bridge_signer: [u8; 20],
        deposits_paused: bool,
    ) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        admission.base_snapshot = Some(CachedBaseMintSnapshot {
            observed_at_ns,
            snapshot,
            bridge_signer,
            deposits_paused,
        });
        admission.refresh_started_at_ns = None;
        self.set_deposit_admission(&admission)
    }

    pub fn fail_base_snapshot_refresh(&mut self) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        admission.refresh_started_at_ns = None;
        self.set_deposit_admission(&admission)
    }

    pub fn signer_address(&self) -> Result<Option<[u8; 20]>, StorageError> {
        Ok(self.deposit_admission()?.signer_address)
    }

    pub fn signer_public_key(&self) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.deposit_admission()?.signer_public_key)
    }

    pub fn set_signer_public_key_if_absent(
        &mut self,
        public_key: Vec<u8>,
    ) -> Result<Vec<u8>, StorageError> {
        let mut admission = self.deposit_admission()?;
        let selected = admission.signer_public_key.unwrap_or(public_key);
        admission.signer_public_key = Some(selected.clone());
        self.set_deposit_admission(&admission)?;
        Ok(selected)
    }

    pub fn set_signer_address_if_absent(
        &mut self,
        address: [u8; 20],
    ) -> Result<[u8; 20], StorageError> {
        let mut admission = self.deposit_admission()?;
        let selected = admission.signer_address.unwrap_or(address);
        admission.signer_address = Some(selected);
        self.set_deposit_admission(&admission)?;
        Ok(selected)
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
        let previous = self
            .fee_payouts
            .get(&value.id)
            .map(|blob| decode::<crate::admin::FeePayoutRecord>(&blob))
            .transpose()?;
        let mut counters = self.counters()?;
        counters.pending_fee_payout_debit = adjust_pending_fee_payout_debit(
            counters.pending_fee_payout_debit,
            previous.as_ref(),
            value,
        )?;
        let value_blob = encode(value)?;
        let counters_blob = encode(&counters)?;
        let previous_key = previous
            .as_ref()
            .map(fee_payout_index_key)
            .transpose()?
            .flatten();
        let next_key = fee_payout_index_key(value)?;
        if let Some(key) = previous_key {
            self.fee_payout_state_index.remove(&key);
        }
        if let Some(key) = next_key {
            self.fee_payout_state_index.insert(key, 0);
        }
        self.fee_payouts.insert(value.id, value_blob);
        self.counters.set(counters_blob);
        Ok(())
    }
    pub fn pending_fee_payout_amount(&self) -> Result<u128, StorageError> {
        Ok(self.counters()?.pending_fee_payout_debit)
    }
    pub fn first_reconcilable_fee_payout(
        &self,
        now_ns: u64,
        dedup_ns: u64,
    ) -> Result<Option<crate::admin::FeePayoutRecord>, StorageError> {
        for entry in self.fee_payout_state_index.iter() {
            let id = fee_payout_id_from_index_key(entry.key())?;
            let value = self
                .fee_payouts
                .get(&id)
                .map(|blob| decode::<crate::admin::FeePayoutRecord>(&blob))
                .transpose()?
                .ok_or(StorageError::RecordNotFound)?;
            if now_ns.saturating_sub(value.transfer.created_at_time_ns) > dedup_ns {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    pub fn complete_fee_payout_success(
        &mut self,
        id: u64,
        block_index: u128,
    ) -> Result<(), StorageError> {
        let mut payout = self
            .fee_payouts
            .get(&id)
            .map(|blob| decode::<crate::admin::FeePayoutRecord>(&blob))
            .transpose()?
            .ok_or(StorageError::RecordNotFound)?;
        match payout.state {
            crate::admin::FeePayoutState::Succeeded {
                block_index: previous,
            } if previous == block_index => return Ok(()),
            crate::admin::FeePayoutState::Pending
            | crate::admin::FeePayoutState::ReconciliationHold => {}
            _ => return Err(StorageError::Core(CoreError::ConflictingReplay)),
        }
        let debit = payout
            .amount
            .checked_add(payout.transfer.fee.get())
            .ok_or(StorageError::CounterOverflow)?;
        let mut accounting = self.accounting()?;
        accounting.spend_fee_reserve(bridge_core::Amount::new(debit))?;
        payout.state = crate::admin::FeePayoutState::Succeeded { block_index };
        let previous_key = fee_payout_index_key_for_state(id, 0)?;
        let accounting_blob = encode(&accounting)?;
        let payout_blob = encode(&payout)?;
        let mut counters = self.counters()?;
        counters.pending_fee_payout_debit = counters
            .pending_fee_payout_debit
            .checked_sub(debit)
            .ok_or(StorageError::CounterUnderflow)?;
        let counters_blob = encode(&counters)?;
        self.accounting.set(accounting_blob);
        self.fee_payouts.insert(id, payout_blob);
        self.fee_payout_state_index.remove(&previous_key);
        self.counters.set(counters_blob);
        Ok(())
    }

    pub fn set_external_progress(&mut self, value: &ExternalProgress) -> Result<(), StorageError> {
        self.external_progress.set(encode(value)?);
        Ok(())
    }

    pub fn put_evm_envelope(&mut self, value: &EvmTransactionEnvelope) -> Result<(), StorageError> {
        if value.operation_ids.is_empty()
            || value.operation_ids.len() > 4
            || value.operation_ids[0] != value.operation_id
        {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        for operation_id in &value.operation_ids {
            if let Some(previous) = self.evm_envelope(operation_id.get())? {
                if previous != *value {
                    let mut expected = value.clone();
                    expected.signed_transaction = previous.signed_transaction.clone();
                    if expected != previous || previous.signed_transaction.is_some() {
                        return Err(StorageError::Core(CoreError::ConflictingReplay));
                    }
                }
            }
        }
        let encoded = encode(value)?;
        for operation_id in &value.operation_ids {
            self.evm_envelopes
                .insert(operation_id.get(), encoded.clone());
        }
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
        let Some(id) = first_evm_index_id(&self.evm_state_index, 1)? else {
            return Ok(None);
        };
        let operation = self
            .evm_operation(id)?
            .ok_or(StorageError::RecordNotFound)?;
        let envelope = self.evm_envelope(id)?.ok_or(StorageError::RecordNotFound)?;
        Ok(Some((operation, envelope)))
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
        let Some(id) = first_evm_index_id(&self.evm_state_index, 0)? else {
            return Ok(None);
        };
        let operation = self
            .evm_operation(id)?
            .ok_or(StorageError::RecordNotFound)?;
        let intent = self
            .evm_call_intent(id)?
            .ok_or(StorageError::RecordNotFound)?;
        Ok(Some((operation, intent)))
    }
    pub fn first_queued_evm_batch(
        &self,
    ) -> Result<Vec<(EvmOperationRecord, EvmCallIntent)>, StorageError> {
        let Some((first, first_intent)) = self.first_queued_evm()? else {
            return Ok(Vec::new());
        };
        let mut batch = vec![(first, first_intent)];
        let start = StableBlob::new(vec![0, first.kind.scheduler_priority()])?;
        let end = StableBlob::new(vec![0, first.kind.scheduler_priority().saturating_add(1)])?;
        for entry in self.evm_state_index.range(start..end) {
            let id = evm_index_id(entry.key())?;
            if id == first.id.get() {
                continue;
            }
            let operation = self
                .evm_operation(id)?
                .ok_or(StorageError::RecordNotFound)?;
            if operation.kind != first.kind {
                continue;
            }
            let intent = self
                .evm_call_intent(id)?
                .ok_or(StorageError::RecordNotFound)?;
            batch.push((operation, intent));
            if batch.len() == 4 {
                break;
            }
        }
        Ok(batch)
    }
    pub fn queued_evm_count(&self) -> Result<u64, StorageError> {
        Ok(self.counters()?.queued_evm_operations)
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
        let value_blob = encode(value)?;
        let counters_blob = encode(&counters)?;
        let operation_owner = deposit_operation_id(value)
            .map(|operation_id| {
                encode(&OperationOwner::Deposit(value.id.bytes()))
                    .map(|owner| (operation_id, owner))
            })
            .transpose()?;
        if let Some((operation_id, owner)) = operation_owner.as_ref() {
            if self
                .operation_owner_index
                .get(operation_id)
                .is_some_and(|previous| previous != *owner)
            {
                return Err(StorageError::Core(CoreError::ConflictingReplay));
            }
        }
        if previous.as_ref().is_some_and(is_pending_deposit_ledger) {
            self.pull_pending_deposit_index.remove(&value.id.bytes());
        }
        if is_pending_deposit_ledger(value) {
            self.pull_pending_deposit_index.insert(value.id.bytes(), 0);
        }
        if let Some((operation_id, owner)) = operation_owner {
            self.operation_owner_index.insert(operation_id, owner);
        }
        self.deposits.insert(value.id.bytes(), value_blob);
        self.counters.set(counters_blob);
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

    pub fn admit_deposit(
        &mut self,
        owner: Principal,
        intent: &DepositIntent,
        record: &DepositRecord,
    ) -> Result<(), StorageError> {
        if self.deposit(record.id.bytes())?.is_some()
            || self.deposit_intent(intent.deposit_id)?.is_some()
        {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        if intent.deposit_id != record.id.bytes() || intent.payload_hash != record.payload_hash {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }

        let mut counters = self.counters()?;
        let sequence = counters.next_deposit_sequence;
        counters.next_deposit_sequence = sequence
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;
        counters.pending_ledger_operations = adjust_active_count(
            counters.pending_ledger_operations,
            false,
            is_pending_deposit_ledger(record),
        )?;
        counters.reserved_deposit_mint_amount =
            adjust_reserved_mint_amount(counters.reserved_deposit_mint_amount, None, record)?;

        let intent_blob = encode(intent)?;
        let record_blob = encode(record)?;
        let counters_blob = encode(&counters)?;
        let index_key = deposit_owner_index_key(owner, sequence)?;

        self.deposit_intents.insert(intent.deposit_id, intent_blob);
        self.deposits.insert(record.id.bytes(), record_blob);
        self.deposit_owner_index
            .insert(index_key, record.id.bytes());
        self.pull_pending_deposit_index.insert(record.id.bytes(), 0);
        self.counters.set(counters_blob);
        Ok(())
    }

    pub fn list_deposit_ids(
        &self,
        owner: Principal,
        before_sequence: Option<u64>,
        limit: u16,
    ) -> Result<(Vec<[u8; 32]>, Option<u64>), StorageError> {
        let prefix = deposit_owner_index_prefix(owner);
        let start_reverse = match before_sequence {
            Some(0) => return Ok((Vec::new(), None)),
            Some(sequence) => u64::MAX
                .checked_sub(sequence)
                .and_then(|value| value.checked_add(1))
                .ok_or(StorageError::CounterOverflow)?,
            None => 0,
        };
        let start = StableBlob::new(deposit_owner_index_bytes(&prefix, start_reverse))?;
        let end = StableBlob::new(deposit_owner_index_bytes(&prefix, u64::MAX))?;
        let mut entries = self
            .deposit_owner_index
            .range(start..=end)
            .take(usize::from(limit) + 1)
            .map(|entry| {
                let sequence = deposit_sequence_from_index_key(entry.key())?;
                Ok((sequence, entry.value()))
            })
            .collect::<Result<Vec<_>, StorageError>>()?;
        let has_more = entries.len() > usize::from(limit);
        if has_more {
            entries.pop();
        }
        let next = has_more
            .then(|| entries.last().map(|entry| entry.0))
            .flatten();
        Ok((entries.into_iter().map(|entry| entry.1).collect(), next))
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
        let Some(entry) = self.pull_pending_deposit_index.iter().next() else {
            return Ok(None);
        };
        self.deposit(*entry.key())
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
        counters.nonterminal_withdrawals = adjust_active_count(
            counters.nonterminal_withdrawals,
            previous.as_ref().is_some_and(is_nonterminal_withdrawal),
            is_nonterminal_withdrawal(value),
        )?;
        let value_blob = encode(value)?;
        let counters_blob = encode(&counters)?;
        let operation_owner = withdrawal_operation_id(value)
            .map(|operation_id| {
                encode(&OperationOwner::Withdrawal(value.id.bytes()))
                    .map(|owner| (operation_id, owner))
            })
            .transpose()?;
        if let Some((operation_id, owner)) = operation_owner.as_ref() {
            if self
                .operation_owner_index
                .get(operation_id)
                .is_some_and(|previous| previous != *owner)
            {
                return Err(StorageError::Core(CoreError::ConflictingReplay));
            }
        }
        if previous.as_ref().is_some_and(is_pending_withdrawal_ledger) {
            self.release_pending_withdrawal_index
                .remove(&value.id.bytes());
        }
        if is_pending_withdrawal_ledger(value) {
            self.release_pending_withdrawal_index
                .insert(value.id.bytes(), 0);
        }
        if let Some((operation_id, owner)) = operation_owner {
            self.operation_owner_index.insert(operation_id, owner);
        }
        self.withdrawals.insert(value.id.bytes(), value_blob);
        self.counters.set(counters_blob);
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
        let Some(entry) = self.release_pending_withdrawal_index.iter().next() else {
            return Ok(None);
        };
        self.withdrawal(*entry.key())
    }

    pub fn nonterminal_withdrawal_count(&self) -> Result<u64, StorageError> {
        Ok(self.counters()?.nonterminal_withdrawals)
    }

    pub fn deposit_for_operation(
        &self,
        operation_id: bridge_core::EvmOperationId,
    ) -> Result<Option<DepositRecord>, StorageError> {
        let Some(owner) = self.operation_owner_index.get(&operation_id.get()) else {
            return Ok(None);
        };
        match decode::<OperationOwner>(&owner)? {
            OperationOwner::Deposit(id) => self.deposit(id),
            OperationOwner::Withdrawal(_) => Ok(None),
        }
    }

    pub fn withdrawal_for_operation(
        &self,
        operation_id: bridge_core::EvmOperationId,
    ) -> Result<Option<WithdrawalRecord>, StorageError> {
        let Some(owner) = self.operation_owner_index.get(&operation_id.get()) else {
            return Ok(None);
        };
        match decode::<OperationOwner>(&owner)? {
            OperationOwner::Withdrawal(id) => self.withdrawal(id),
            OperationOwner::Deposit(_) => Ok(None),
        }
    }

    pub fn first_submitted_evm(&self) -> Result<Option<EvmOperationRecord>, StorageError> {
        let Some(id) = first_evm_index_id(&self.evm_state_index, 2)? else {
            return Ok(None);
        };
        self.evm_operation(id)
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
        counters.queued_evm_operations = adjust_active_count(
            counters.queued_evm_operations,
            previous
                .as_ref()
                .is_some_and(|operation| matches!(operation.state, EvmOperationState::Queued)),
            matches!(value.state, EvmOperationState::Queued),
        )?;
        let encoded_counters = encode(&counters)?;
        if let Some(previous_key) = previous
            .as_ref()
            .map(evm_state_index_key)
            .transpose()?
            .flatten()
        {
            self.evm_state_index.remove(&previous_key);
        }
        if let Some(next_key) = evm_state_index_key(value)? {
            self.evm_state_index.insert(next_key, 0);
        }
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

    pub fn enqueue_withdrawal_notification(
        &mut self,
        caller: Principal,
        transaction_hash: [u8; 32],
        now_ns: u64,
    ) -> Result<NotificationEnqueueOutcome, NotificationEnqueueError> {
        const QUEUE_LIMIT: u64 = 64;
        const CALLER_PENDING_LIMIT: usize = 4;
        const WINDOW_NS: u64 = 10 * 60 * 1_000_000_000;
        const GLOBAL_WINDOW_LIMIT: u8 = 32;
        const CALLER_WINDOW_LIMIT: u8 = 4;

        if self
            .withdrawal_notifications
            .contains_key(&transaction_hash)
        {
            return Ok(NotificationEnqueueOutcome::Duplicate);
        }
        if self.withdrawal_notifications.len() >= QUEUE_LIMIT {
            return Err(NotificationEnqueueError::QueueFull);
        }
        let pending_for_caller = self
            .withdrawal_notifications
            .iter()
            .map(|entry| decode::<WithdrawalNotification>(&entry.value()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| NotificationEnqueueError::Storage)?
            .into_iter()
            .filter(|notification| notification.caller == caller)
            .count();
        if pending_for_caller >= CALLER_PENDING_LIMIT {
            return Err(NotificationEnqueueError::RateLimited {
                retry_after_seconds: 600,
            });
        }

        let window_id = now_ns / WINDOW_NS;
        let mut control =
            decode::<WithdrawalNotificationControl>(self.withdrawal_notification_control.get())
                .map_err(|_| NotificationEnqueueError::Storage)?;
        if control.window_id != window_id {
            control = WithdrawalNotificationControl {
                window_id,
                ..WithdrawalNotificationControl::default()
            };
        }
        let caller_count = control
            .caller_counts
            .iter()
            .find(|quota| quota.caller == caller)
            .map(|quota| quota.count)
            .unwrap_or(0);
        if control.global_count >= GLOBAL_WINDOW_LIMIT || caller_count >= CALLER_WINDOW_LIMIT {
            let retry_after_ns = WINDOW_NS.saturating_sub(now_ns % WINDOW_NS);
            return Err(NotificationEnqueueError::RateLimited {
                retry_after_seconds: retry_after_ns.saturating_add(999_999_999) / 1_000_000_000,
            });
        }

        control.global_count = control
            .global_count
            .checked_add(1)
            .ok_or(NotificationEnqueueError::Storage)?;
        match control
            .caller_counts
            .iter_mut()
            .find(|quota| quota.caller == caller)
        {
            Some(quota) => {
                quota.count = quota
                    .count
                    .checked_add(1)
                    .ok_or(NotificationEnqueueError::Storage)?;
            }
            None => control
                .caller_counts
                .push(NotificationCallerQuota { caller, count: 1 }),
        }

        let notification = WithdrawalNotification {
            transaction_hash,
            caller,
            created_at_ns: now_ns,
            next_attempt_at_ns: now_ns,
            attempts: 0,
        };
        let encoded_notification =
            encode(&notification).map_err(|_| NotificationEnqueueError::Storage)?;
        let encoded_control = encode(&control).map_err(|_| NotificationEnqueueError::Storage)?;
        self.withdrawal_notifications
            .insert(transaction_hash, encoded_notification);
        self.withdrawal_notification_control.set(encoded_control);
        Ok(NotificationEnqueueOutcome::Queued)
    }

    pub fn first_due_withdrawal_notification(
        &self,
        now_ns: u64,
    ) -> Result<Option<WithdrawalNotification>, StorageError> {
        let mut selected: Option<WithdrawalNotification> = None;
        for entry in self.withdrawal_notifications.iter() {
            let candidate: WithdrawalNotification = decode(&entry.value())?;
            if candidate.next_attempt_at_ns > now_ns {
                continue;
            }
            if selected
                .as_ref()
                .is_none_or(|current| candidate.created_at_ns < current.created_at_ns)
            {
                selected = Some(candidate);
            }
        }
        Ok(selected)
    }

    pub fn put_withdrawal_notification(
        &mut self,
        value: &WithdrawalNotification,
    ) -> Result<(), StorageError> {
        let previous = self
            .withdrawal_notifications
            .get(&value.transaction_hash)
            .map(|blob| decode::<WithdrawalNotification>(&blob))
            .transpose()?
            .ok_or(StorageError::RecordNotFound)?;
        if previous.caller != value.caller || previous.created_at_ns != value.created_at_ns {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        self.withdrawal_notifications
            .insert(value.transaction_hash, encode(value)?);
        Ok(())
    }

    pub fn remove_withdrawal_notification(&mut self, transaction_hash: [u8; 32]) {
        self.withdrawal_notifications.remove(&transaction_hash);
    }

    pub fn withdrawal_notification_count(&self) -> u64 {
        self.withdrawal_notifications.len()
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
        if previous.as_ref().is_some_and(is_open_hold) {
            self.open_hold_index.remove(&value.id.get());
        }
        if is_open_hold(value) {
            self.open_hold_index.insert(value.id.get(), 0);
        }
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
        let Some(entry) = self.open_hold_index.iter().next() else {
            return Ok(None);
        };
        self.reconciliation_hold(*entry.key())
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
        let previous_hold = self.reconciliation_hold(hold.id.get())?;
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
        if previous_deposit
            .as_ref()
            .is_some_and(is_pending_deposit_ledger)
        {
            self.pull_pending_deposit_index.remove(&deposit.id.bytes());
        }
        if is_pending_deposit_ledger(deposit) {
            self.pull_pending_deposit_index
                .insert(deposit.id.bytes(), 0);
        }
        if previous_hold.as_ref().is_some_and(is_open_hold) {
            self.open_hold_index.remove(&hold.id.get());
        }
        if is_open_hold(hold) {
            self.open_hold_index.insert(hold.id.get(), 0);
        }
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
        let previous_hold = self.reconciliation_hold(hold.id.get())?;
        let mut counters = self.counters_after_hold_update(hold)?;
        counters.pending_ledger_operations = adjust_active_count(
            counters.pending_ledger_operations,
            previous_withdrawal
                .as_ref()
                .map(is_pending_withdrawal_ledger)
                .unwrap_or(false),
            is_pending_withdrawal_ledger(withdrawal),
        )?;
        counters.nonterminal_withdrawals = adjust_active_count(
            counters.nonterminal_withdrawals,
            previous_withdrawal
                .as_ref()
                .is_some_and(is_nonterminal_withdrawal),
            is_nonterminal_withdrawal(withdrawal),
        )?;
        let encoded_counters = encode(&counters)?;
        if previous_withdrawal
            .as_ref()
            .is_some_and(is_pending_withdrawal_ledger)
        {
            self.release_pending_withdrawal_index
                .remove(&withdrawal.id.bytes());
        }
        if is_pending_withdrawal_ledger(withdrawal) {
            self.release_pending_withdrawal_index
                .insert(withdrawal.id.bytes(), 0);
        }
        if previous_hold.as_ref().is_some_and(is_open_hold) {
            self.open_hold_index.remove(&hold.id.get());
        }
        if is_open_hold(hold) {
            self.open_hold_index.insert(hold.id.get(), 0);
        }
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

fn is_nonterminal_withdrawal(value: &WithdrawalRecord) -> bool {
    !matches!(
        value.state,
        WithdrawalState::Released { .. } | WithdrawalState::Refunded { .. }
    )
}

fn is_pending_fee_payout(value: &crate::admin::FeePayoutRecord) -> bool {
    matches!(
        value.state,
        crate::admin::FeePayoutState::Pending | crate::admin::FeePayoutState::ReconciliationHold
    )
}

fn fee_payout_debit(value: &crate::admin::FeePayoutRecord) -> Result<u128, StorageError> {
    value
        .amount
        .checked_add(value.transfer.fee.get())
        .ok_or(StorageError::CounterOverflow)
}

fn adjust_pending_fee_payout_debit(
    current: u128,
    previous: Option<&crate::admin::FeePayoutRecord>,
    next: &crate::admin::FeePayoutRecord,
) -> Result<u128, StorageError> {
    let without_previous = if previous.is_some_and(is_pending_fee_payout) {
        current
            .checked_sub(fee_payout_debit(previous.expect("checked previous"))?)
            .ok_or(StorageError::CounterUnderflow)?
    } else {
        current
    };
    if is_pending_fee_payout(next) {
        without_previous
            .checked_add(fee_payout_debit(next)?)
            .ok_or(StorageError::CounterOverflow)
    } else {
        Ok(without_previous)
    }
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
        RequestReference, Settlement, TransferAttempt, WithdrawalEvent, WithdrawalHoldResolution,
        WithdrawalId,
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
            deposit_rate_limit_window_seconds: 60,
            deposit_rate_limit_global: 30,
            deposit_rate_limit_per_principal: 3,
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

    fn intent(id: [u8; 32], owner: Principal) -> DepositIntent {
        DepositIntent {
            deposit_id: id,
            caller: owner.as_slice().to_vec(),
            client_request_id: id,
            base_recipient: [9; 20],
            from_subaccount: [0; 32],
            payload_hash: [2; 32],
        }
    }

    #[test]
    fn deposit_owner_index_is_newest_first_paginated_and_owner_scoped() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize stable store");
        let owner = Principal::self_authenticating([1; 32]);
        let other = Principal::self_authenticating([2; 32]);

        for (tag, principal) in [(1u8, owner), (2, other), (3, owner), (4, owner)] {
            let mut record = deposit();
            record.id = DepositId::new([tag; 32]);
            record.payload_hash = [2; 32];
            store
                .admit_deposit(principal, &intent([tag; 32], principal), &record)
                .expect("admit deposit");
        }

        let (first, cursor) = store
            .list_deposit_ids(owner, None, 2)
            .expect("list first page");
        assert_eq!(first, vec![[4; 32], [3; 32]]);
        assert_eq!(cursor, Some(2));

        let (second, cursor) = store
            .list_deposit_ids(owner, cursor, 2)
            .expect("list second page");
        assert_eq!(second, vec![[1; 32]]);
        assert_eq!(cursor, None);
        assert_eq!(
            store
                .list_deposit_ids(other, None, 100)
                .expect("list other owner")
                .0,
            vec![[2; 32]]
        );
    }

    #[test]
    fn deposit_admission_rejects_replay_without_duplicate_index_entry() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize stable store");
        let owner = Principal::self_authenticating([3; 32]);
        let record = deposit();
        let intent = intent(record.id.bytes(), owner);
        store
            .admit_deposit(owner, &intent, &record)
            .expect("first admission");
        assert!(matches!(
            store.admit_deposit(owner, &intent, &record),
            Err(StorageError::Core(CoreError::ConflictingReplay))
        ));
        assert_eq!(
            store
                .list_deposit_ids(owner, None, 100)
                .expect("list owner deposits")
                .0,
            vec![record.id.bytes()]
        );
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
                last_finalized_base_block: 0,
            }
        );
    }

    #[test]
    fn schema_wire_corruption_and_size_are_rejected() {
        #[derive(Serialize)]
        struct IncompleteCounterState {
            next_evm_operation_id: u64,
            next_hold_id: u64,
            pending_evm_operations: u64,
            reconciliation_holds: u64,
        }

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
        assert_eq!(
            decode::<CounterState>(
                &encode(&IncompleteCounterState {
                    next_evm_operation_id: 0,
                    next_hold_id: 0,
                    pending_evm_operations: 0,
                    reconciliation_holds: 0,
                })
                .expect("encode incomplete value")
            ),
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
    fn non_current_schema_is_rejected_without_migration() {
        let memory = VectorMemory::default();
        let manager = MemoryManager::init(memory.clone());
        StableCell::init(manager.get(SCHEMA_MEMORY_ID), 2u16);
        assert!(matches!(
            StableStore::init(memory),
            Err(StorageError::UnsupportedSchemaVersion(2))
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
            receipt_block_number: 2,
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
            receipt_block_number: 98,
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
                receipt_block_number: id,
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
            receipt_block_number: 2,
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
    fn deposit_quota_is_principal_scoped_and_resets_by_window() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let caller = Principal::self_authenticating([1; 32]);
        let other = Principal::self_authenticating([2; 32]);
        store
            .reserve_deposit_quota(caller, 1, 60, 3, 2)
            .expect("first admission");
        store
            .reserve_deposit_quota(caller, 2, 60, 3, 2)
            .expect("second admission");
        assert_eq!(
            store.reserve_deposit_quota(caller, 3, 60, 3, 2),
            Err(DepositQuotaError::RateLimited(DepositRateLimit {
                retry_after_seconds: 60
            }))
        );
        store
            .reserve_deposit_quota(other, 4, 60, 3, 2)
            .expect("global final slot");
        assert!(store
            .reserve_deposit_quota(Principal::self_authenticating([3; 32]), 5, 60, 3, 2)
            .is_err());
        store
            .reserve_deposit_quota(caller, 60_000_000_000, 60, 3, 2)
            .expect("new window resets quota");
    }

    #[test]
    fn withdrawal_notification_queue_is_deduplicated_bounded_and_rate_limited() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let caller = Principal::self_authenticating([1; 32]);
        assert_eq!(
            store.enqueue_withdrawal_notification(caller, [1; 32], 1),
            Ok(NotificationEnqueueOutcome::Queued)
        );
        assert_eq!(
            store.enqueue_withdrawal_notification(caller, [1; 32], 2),
            Ok(NotificationEnqueueOutcome::Duplicate)
        );
        for tag in 2..=4 {
            assert_eq!(
                store.enqueue_withdrawal_notification(caller, [tag; 32], u64::from(tag)),
                Ok(NotificationEnqueueOutcome::Queued)
            );
        }
        assert!(matches!(
            store.enqueue_withdrawal_notification(caller, [5; 32], 5),
            Err(NotificationEnqueueError::RateLimited { .. })
        ));
        assert_eq!(store.withdrawal_notification_count(), 4);
        assert_eq!(
            store
                .first_due_withdrawal_notification(5)
                .expect("read due notification")
                .expect("notification")
                .transaction_hash,
            [1; 32]
        );
    }

    #[test]
    fn withdrawal_notification_global_window_limit_is_enforced() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        for tag in 1..=32u8 {
            let caller = Principal::self_authenticating([tag; 32]);
            assert_eq!(
                store.enqueue_withdrawal_notification(caller, [tag; 32], 1),
                Ok(NotificationEnqueueOutcome::Queued)
            );
        }
        assert!(matches!(
            store.enqueue_withdrawal_notification(
                Principal::self_authenticating([99; 32]),
                [99; 32],
                1
            ),
            Err(NotificationEnqueueError::RateLimited { .. })
        ));
    }

    #[test]
    fn base_snapshot_cache_is_bounded_by_ttl_progress_and_singleflight() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let snapshot = BaseMintSnapshot {
            finalized_block_number: 10,
            finalized_block_timestamp: 10,
            service_fee: Amount::new(1),
            max_service_fee: Amount::new(2),
            per_deposit_limit: Amount::new(100),
            mint_window_limit: Amount::new(1_000),
            mint_window_started_at: 0,
            mint_window_duration: 100,
            minted_in_window: Amount::ZERO,
        };
        assert!(store
            .begin_base_snapshot_refresh(100, 300, 60)
            .expect("begin refresh"));
        assert!(!store
            .begin_base_snapshot_refresh(101, 300, 60)
            .expect("singleflight rejects overlap"));
        store
            .finish_base_snapshot_refresh(110, snapshot, [7; 20], false)
            .expect("cache snapshot");
        let cached = store
            .cached_base_mint_snapshot(160, 60, 10)
            .expect("fresh cache")
            .expect("cached snapshot");
        assert_eq!(cached.snapshot, snapshot);
        assert_eq!(cached.bridge_signer, [7; 20]);
        assert!(!cached.deposits_paused);
        assert_eq!(
            store
                .cached_base_mint_snapshot(171, 60, 10)
                .expect("expired cache"),
            None
        );
        assert_eq!(
            store
                .cached_base_mint_snapshot(120, 60, 11)
                .expect("progress-invalid cache"),
            None
        );
    }

    #[test]
    fn fee_payout_success_debits_once_and_removes_reconciliation_reservation() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        store
            .set_accounting(&AccountingState {
                fee_reserve: Amount::new(101),
                ..AccountingState::default()
            })
            .expect("seed reserve");
        let recipient = FeeRecipientConfig {
            owner: Principal::self_authenticating([7; 32]),
            subaccount: vec![],
        };
        let payout = crate::admin::FeePayoutRecord {
            id: 4,
            amount: 100,
            recipient,
            transfer: transfer(LedgerOperation::FeePayout, 100, 30),
            state: crate::admin::FeePayoutState::Pending,
        };
        store.put_fee_payout(&payout).expect("insert payout");
        assert_eq!(store.pending_fee_payout_amount().expect("pending"), 101);
        assert_eq!(
            store
                .first_reconcilable_fee_payout(1_000, 100)
                .expect("aged pending payout"),
            Some(payout.clone())
        );
        store
            .complete_fee_payout_success(payout.id, 8)
            .expect("complete payout");
        store
            .complete_fee_payout_success(payout.id, 8)
            .expect("idempotent replay");
        assert_eq!(
            store.accounting().expect("accounting").fee_reserve,
            Amount::ZERO
        );
        assert_eq!(store.pending_fee_payout_amount().expect("pending"), 0);
        assert_eq!(
            store.complete_fee_payout_success(payout.id, 9),
            Err(StorageError::Core(CoreError::ConflictingReplay))
        );
    }

    #[test]
    fn reserved_memory_ids_are_never_reassigned() {
        assert_eq!(RESERVED_MEMORY_IDS, 26..=31);
    }
}
