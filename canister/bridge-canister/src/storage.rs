use bridge_core::{
    resolve_deposit_hold, resolve_withdrawal_hold, AccountingState, ApplyResult, CoreError,
    DepositId, DepositRecord, EvmOperationRecord, EvmOperationState, HoldId, HoldResolution,
    ReconciliationHoldRecord, ReconciliationHoldState, WithdrawalId, WithdrawalRecord,
};
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
pub const RESERVED_MEMORY_IDS: core::ops::RangeInclusive<u8> = 7..=15;

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageCounts {
    pub deposits: u64,
    pub withdrawals: u64,
    pub pending_evm_operations: u64,
    pub reconciliation_holds: u64,
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

    #[cfg(test)]
    fn set_counters(&mut self, value: &CounterState) -> Result<(), StorageError> {
        self.counters.set(encode(value)?);
        Ok(())
    }

    pub fn put_deposit(&mut self, value: &DepositRecord) -> Result<(), StorageError> {
        self.deposits.insert(value.id.bytes(), encode(value)?);
        Ok(())
    }

    pub fn deposit(&self, id: [u8; 32]) -> Result<Option<DepositRecord>, StorageError> {
        self.deposits.get(&id).map(|blob| decode(&blob)).transpose()
    }

    pub fn put_withdrawal(&mut self, value: &WithdrawalRecord) -> Result<(), StorageError> {
        self.withdrawals.insert(value.id.bytes(), encode(value)?);
        Ok(())
    }

    pub fn withdrawal(&self, id: [u8; 32]) -> Result<Option<WithdrawalRecord>, StorageError> {
        self.withdrawals
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
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

    pub fn resolve_deposit_hold(
        &mut self,
        deposit_id: DepositId,
        hold_id: HoldId,
        resolution: HoldResolution,
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
        resolution: HoldResolution,
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
        let encoded_counters = self.counters_after_hold_update(hold)?;
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
        let encoded_counters = self.counters_after_hold_update(hold)?;
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
    ) -> Result<StableBlob, StorageError> {
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
        encode(&counters)
    }

    pub fn status_counts(&self) -> Result<StorageCounts, StorageError> {
        let counters = self.counters()?;

        Ok(StorageCounts {
            deposits: self.deposits.len(),
            withdrawals: self.withdrawals.len(),
            pending_evm_operations: counters.pending_evm_operations,
            reconciliation_holds: counters.reconciliation_holds,
        })
    }
}

fn is_pending_evm(value: &EvmOperationRecord) -> bool {
    !matches!(value.state, EvmOperationState::Finalized { .. })
}

fn is_open_hold(value: &ReconciliationHoldRecord) -> bool {
    matches!(value.state, ReconciliationHoldState::Open)
}

fn adjust_active_count(
    current: u64,
    was_active: bool,
    is_active: bool,
) -> Result<u64, StorageError> {
    match (was_active, is_active) {
        (false, true) => current.checked_add(1).ok_or(StorageError::CounterOverflow),
        (true, false) => current.checked_sub(1).ok_or(StorageError::CounterUnderflow),
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
        Account, Amount, ApplyOutcome, BaseMintSnapshot, DepositEvent, DepositId, DepositRequest,
        DepositState, EvmOperationId, EvmOperationKind, HoldId, HoldResolution, LedgerOperation,
        LedgerTransferIdentity, ReconciliationHoldRecord, ReconciliationHoldState,
        RequestReference, ResourceBudget, ResourceCost, Settlement, WithdrawalEvent, WithdrawalId,
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
                service_fee: Amount::new(10),
                max_service_fee: Amount::new(20),
                per_deposit_limit: Amount::new(1_000),
                mint_window_limit: Amount::new(10_000),
                minted_in_window: Amount::ZERO,
            },
            ResourceBudget {
                available: ResourceCost {
                    eth_wei: 10,
                    cycles: 10,
                },
                settlement_floor: ResourceCost::default(),
                pending_settlements: ResourceCost::default(),
            },
            ResourceCost {
                eth_wei: 1,
                cycles: 1,
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
                transfer: Box::new(transfer(LedgerOperation::ReleaseWithdrawal, 85, 20)),
                settlement: Settlement {
                    amount_out: Amount::new(85),
                    service_fee: Amount::new(10),
                    ledger_fee: Amount::new(5),
                },
            })
            .expect("release pending");
        withdrawal
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
                service_fee: Amount::new(10),
                max_service_fee: Amount::new(20),
                per_deposit_limit: Amount::new(1_000),
                mint_window_limit: Amount::new(10_000),
                minted_in_window: Amount::ZERO,
            },
            ResourceBudget {
                available: ResourceCost {
                    eth_wei: 10,
                    cycles: 10,
                },
                settlement_floor: ResourceCost::default(),
                pending_settlements: ResourceCost::default(),
            },
            ResourceCost {
                eth_wei: 1,
                cycles: 1,
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
        let evm = EvmOperationRecord::prepared(
            EvmOperationId::new(6),
            [6; 32],
            EvmOperationKind::AcknowledgeRelease,
        );
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
                        HoldResolution::Succeeded {
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
                    HoldResolution::Succeeded {
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
            bridge_core::WithdrawalState::ReconciliationHold { transfer, .. } => transfer.clone(),
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
        let resolution = HoldResolution::Absent {
            history_watermark: Some(500),
        };
        assert_eq!(
            store
                .resolve_withdrawal_hold(withdrawal.id, hold.id, resolution)
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
        assert_eq!(RESERVED_MEMORY_IDS, 7..=15);
    }
}
