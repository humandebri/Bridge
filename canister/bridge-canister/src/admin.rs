use crate::{config::FeeRecipientConfig, ledger, storage::AuditEventPage, STORE};
use bridge_core::{Account, Amount, LedgerCallOutcome, LedgerOperation, LedgerTransferIdentity};
use candid::{CandidType, Deserialize, Nat, Principal};
use serde::Serialize;
use sha2::{Digest, Sha256};

const ACTION_PAUSE: u8 = 0;
const ACTION_RESUME: u8 = 1;
const ACTION_RECIPIENT: u8 = 2;
const ACTION_PAYOUT: u8 = 3;
const ACTION_ROTATE: u8 = 4;

fn authorized(state: &AdminState, caller: Principal, action: u8) -> bool {
    bridge_core::administrator_authorized(
        action,
        state.pause_principal == caller,
        state.governance_principal == caller,
    )
}

pub(crate) fn can_advance_settlement(caller: Principal) -> Result<bool, AdminError> {
    if caller == Principal::anonymous() {
        return Ok(false);
    }
    STORE.with(|store| {
        let state = store
            .borrow()
            .admin_state()
            .map_err(|_| AdminError::StorageFailure)?;
        Ok(state.governance_principal == caller || state.pause_principal == caller)
    })
}

pub(crate) fn is_governance(caller: Principal) -> Result<bool, AdminError> {
    if caller == Principal::anonymous() {
        return Ok(false);
    }
    STORE.with(|store| {
        let state = store
            .borrow()
            .admin_state()
            .map_err(|_| AdminError::StorageFailure)?;
        Ok(state.governance_principal == caller)
    })
}

pub(crate) fn can_manage_fee_payout(caller: Principal) -> Result<bool, AdminError> {
    is_governance(caller)
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct AdminState {
    pub deposits_paused: bool,
    pub withdrawal_fee_guard: Option<WithdrawalFeeGuard>,
    pub pause_principal: Principal,
    pub governance_principal: Principal,
    pub fee_recipient: FeeRecipientConfig,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WithdrawalFeeGuard {
    pub ledger_fee: u128,
    pub charged_service_fee: u128,
    pub tripped_at_ns: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RotatePausePrincipalArgs {
    pub pause_principal: Principal,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum AdminError {
    Busy,
    Unauthorized,
    InvalidArgument(String),
    StorageFailure,
    InsufficientFeeReserve,
    UnresolvedEvmRevert,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum FeePayoutState {
    Pending,
    Succeeded { block_index: u128 },
    ReconciliationHold,
    Failed,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FeePayoutRecord {
    pub id: u64,
    pub amount: u128,
    pub recipient: FeeRecipientConfig,
    pub transfer: LedgerTransferIdentity,
    pub state: FeePayoutState,
}
#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FeePayoutReceipt {
    pub id: u64,
    pub amount: Nat,
    pub state: FeePayoutState,
}

fn mutate(
    caller: Principal,
    action: impl FnOnce(&mut AdminState) -> Result<crate::storage::AuditEventKind, AdminError>,
) -> Result<(), AdminError> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut state = store
            .admin_state()
            .map_err(|_| AdminError::StorageFailure)?;
        let event = action(&mut state)?;
        store
            .set_admin_state(&state)
            .map_err(|_| AdminError::StorageFailure)?;
        store
            .append_audit_event(caller, event)
            .unwrap_or_else(|error| ic_cdk::trap(format!("audit persistence failed: {error}")));
        Ok(())
    })
}

pub fn pause(caller: Principal) -> Result<(), AdminError> {
    mutate(caller, |state| {
        if !authorized(state, caller, ACTION_PAUSE) {
            return Err(AdminError::Unauthorized);
        }
        if state.deposits_paused {
            return Ok(crate::storage::AuditEventKind::DepositsPauseRepeated);
        }
        state.deposits_paused = true;
        Ok(crate::storage::AuditEventKind::DepositsPaused)
    })
}

pub fn resume(caller: Principal) -> Result<(), AdminError> {
    let unresolved = STORE.with(|store| {
        store
            .borrow()
            .counters()
            .map(|counters| counters.unresolved_evm_reverts != 0)
            .map_err(|_| AdminError::StorageFailure)
    })?;
    if unresolved {
        return Err(AdminError::UnresolvedEvmRevert);
    }
    mutate(caller, |state| {
        if !authorized(state, caller, ACTION_RESUME) {
            return Err(AdminError::Unauthorized);
        }
        state.deposits_paused = false;
        Ok(crate::storage::AuditEventKind::DepositsResumed)
    })
}

pub fn set_fee_recipient(caller: Principal, value: FeeRecipientConfig) -> Result<(), AdminError> {
    if value.owner == Principal::anonymous() || !matches!(value.subaccount.len(), 0 | 32) {
        return Err(AdminError::InvalidArgument("invalid fee recipient".into()));
    }
    mutate(caller, |state| {
        if !authorized(state, caller, ACTION_RECIPIENT) {
            return Err(AdminError::Unauthorized);
        }
        let previous = state.fee_recipient.clone();
        state.fee_recipient = value.clone();
        Ok(crate::storage::AuditEventKind::FeeRecipientChanged {
            previous,
            current: value,
        })
    })
}

pub fn rotate_pause_principal(
    caller: Principal,
    args: RotatePausePrincipalArgs,
) -> Result<(), AdminError> {
    if args.pause_principal == Principal::anonymous() {
        return Err(AdminError::InvalidArgument(
            "invalid pause principal".into(),
        ));
    }
    mutate(caller, |state| {
        if !authorized(state, caller, ACTION_ROTATE) {
            return Err(AdminError::Unauthorized);
        }
        if args.pause_principal == state.governance_principal
            || args.pause_principal == state.fee_recipient.owner
        {
            return Err(AdminError::InvalidArgument(
                "pause principal must not overlap governance or fee recipient".into(),
            ));
        }
        state.pause_principal = args.pause_principal;
        Ok(crate::storage::AuditEventKind::PausePrincipalRotated)
    })
}

pub fn audit_events(start: u64, limit: u16) -> Result<AuditEventPage, AdminError> {
    if !(1..=100).contains(&limit) {
        return Err(AdminError::InvalidArgument("limit must be 1..=100".into()));
    }
    STORE.with(|store| {
        store
            .borrow()
            .audit_events(start, limit)
            .map_err(|_| AdminError::StorageFailure)
    })
}

pub async fn request_fee_payout(
    caller: Principal,
    amount: Nat,
) -> Result<FeePayoutReceipt, AdminError> {
    let amount: u128 = amount
        .0
        .to_string()
        .parse()
        .map_err(|_| AdminError::InvalidArgument("amount exceeds u128".into()))?;
    if amount == 0 {
        return Err(AdminError::InvalidArgument("amount must be nonzero".into()));
    }
    let config = STORE.with(|store| {
        let store = store.borrow();
        let admin = store
            .admin_state()
            .map_err(|_| AdminError::StorageFailure)?;
        if !authorized(&admin, caller, ACTION_PAYOUT) {
            return Err(AdminError::Unauthorized);
        }
        store
            .config()
            .map_err(|_| AdminError::StorageFailure)?
            .ok_or(AdminError::StorageFailure)
    })?;
    let fee = ledger::ledger_fee(config.ledger_canister_id)
        .await
        .map_err(|_| AdminError::StorageFailure)?;
    let mut record = STORE.with(|store| {
        let mut store = store.borrow_mut();
        let admin = store
            .admin_state()
            .map_err(|_| AdminError::StorageFailure)?;
        if !authorized(&admin, caller, ACTION_PAYOUT) {
            return Err(AdminError::Unauthorized);
        }
        let reserved = store
            .pending_fee_payout_amount()
            .map_err(|_| AdminError::StorageFailure)?;
        let reserve = store
            .accounting()
            .map_err(|_| AdminError::StorageFailure)?
            .fee_reserve
            .get();
        if !bridge_core::payout_allowed(reserve, reserved, amount, fee.get()) {
            return Err(AdminError::InsufficientFeeReserve);
        }
        let id = store
            .next_fee_payout_id()
            .map_err(|_| AdminError::StorageFailure)?;
        let mut digest = Sha256::new();
        digest.update(b"KINIC-FEE-PAYOUT");
        digest.update(id.to_be_bytes());
        let memo: [u8; 32] = digest.finalize().into();
        let subaccount: [u8; 32] = if admin.fee_recipient.subaccount.is_empty() {
            [0; 32]
        } else {
            admin
                .fee_recipient
                .subaccount
                .as_slice()
                .try_into()
                .map_err(|_| AdminError::InvalidArgument("invalid subaccount".into()))?
        };
        let canister = ic_cdk::api::canister_self();
        let created_at_time_ns = ic_cdk::api::time();
        let transfer = LedgerTransferIdentity {
            operation: LedgerOperation::FeePayout,
            created_at_time_ns,
            memo,
            amount: Amount::new(amount),
            fee,
            from: Account::new(canister.as_slice().to_vec(), [0; 32])
                .map_err(|_| AdminError::StorageFailure)?,
            to: Account::new(admin.fee_recipient.owner.as_slice().to_vec(), subaccount)
                .map_err(|_| AdminError::InvalidArgument("invalid recipient".into()))?,
            spender: None,
        };
        let record = FeePayoutRecord {
            id,
            amount,
            recipient: admin.fee_recipient,
            transfer,
            state: FeePayoutState::Pending,
        };
        store
            .commit_fee_payout_request(&record, caller, created_at_time_ns)
            .map_err(|_| AdminError::StorageFailure)?;
        Ok(record)
    })?;
    match ledger::release(config.ledger_canister_id, &record.transfer).await {
        LedgerCallOutcome::Succeeded { block_index }
        | LedgerCallOutcome::Duplicate { block_index } => {
            record.state = FeePayoutState::Succeeded { block_index };
            STORE.with(|store| {
                store
                    .borrow_mut()
                    .complete_fee_payout_success(record.id, block_index)
                    .map_err(|_| AdminError::StorageFailure)
            })?;
        }
        LedgerCallOutcome::Ambiguous => {
            record.state = FeePayoutState::ReconciliationHold;
            STORE.with(|store| {
                store
                    .borrow_mut()
                    .hold_fee_payout(record.id)
                    .map_err(|_| AdminError::StorageFailure)
            })?;
        }
        LedgerCallOutcome::DefinitiveFailure { .. } => {
            record.state = FeePayoutState::Failed;
            STORE.with(|store| {
                store
                    .borrow_mut()
                    .complete_fee_payout_failure(record.id)
                    .map_err(|_| AdminError::StorageFailure)
            })?;
        }
        LedgerCallOutcome::RetryableFailure { .. } => {
            // Keep the record pending. A later attempt requires an explicit
            // continue_fee_payout call from an authorized administrator.
        }
    }
    Ok(FeePayoutReceipt {
        id: record.id,
        amount: Nat::from(record.amount),
        state: record.state,
    })
}
