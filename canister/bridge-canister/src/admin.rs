use crate::{config::FeeRecipientConfig, ledger, storage::AuditEventPage, STORE};
use bridge_core::{Account, Amount, LedgerOperation, LedgerTransferIdentity};
use candid::{CandidType, Deserialize, Nat, Principal};
use serde::Serialize;
use sha2::{Digest, Sha256};

const ACTION_PAUSE: u8 = 0;
const ACTION_RESUME: u8 = 1;
const ACTION_PAYOUT: u8 = 2;
const ACTION_ROTATE: u8 = 3;

fn authorized(state: &AdminState, caller: Principal, action: u8) -> bool {
    bridge_core::administrator_authorized(
        action,
        state.pause_principal == caller,
        state.governance_principal == caller,
    )
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

fn fee_recipient_digest(value: &FeeRecipientConfig) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"KINIC-FEE-RECIPIENT");
    digest.update(value.owner.as_slice());
    digest.update((value.subaccount.len() as u64).to_be_bytes());
    digest.update(&value.subaccount);
    digest.finalize().into()
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum AdminError {
    Busy,
    Unauthorized,
    InvalidArgument(String),
    StorageFailure,
    InsufficientFeeReserve,
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
) -> Result<crate::storage::AuditEvent, AdminError> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut state = store
            .admin_state()
            .map_err(|_| AdminError::StorageFailure)?;
        let event = action(&mut state)?;
        store
            .set_admin_state(&state)
            .map_err(|_| AdminError::StorageFailure)?;
        let audit = store
            .append_audit_event(caller, event)
            .unwrap_or_else(|error| ic_cdk::trap(format!("audit persistence failed: {error}")));
        Ok(audit)
    })
}

pub fn pause(caller: Principal) -> Result<(), AdminError> {
    pause_with_audit(caller).map(drop)
}

pub fn pause_with_audit(caller: Principal) -> Result<crate::storage::AuditEvent, AdminError> {
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

pub(crate) fn confirmed_activation_resume_authorized(
    state: &AdminState,
    caller: Principal,
) -> bool {
    authorized(state, caller, ACTION_RESUME)
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
    let confirmation_relayer = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| AdminError::StorageFailure)?
            .map(|config| config.confirmation_relayer_principal)
            .ok_or(AdminError::StorageFailure)
    })?;
    mutate(caller, |state| {
        if !authorized(state, caller, ACTION_ROTATE) {
            return Err(AdminError::Unauthorized);
        }
        if args.pause_principal == state.governance_principal
            || args.pause_principal == state.fee_recipient.owner
            || args.pause_principal == confirmation_relayer
        {
            return Err(AdminError::InvalidArgument(
                "pause principal must not overlap governance, fee recipient, or confirmation relayer".into(),
            ));
        }
        state.pause_principal = args.pause_principal;
        Ok(crate::storage::AuditEventKind::PausePrincipalRotated)
    })
    .map(drop)
}

pub fn rotate_fee_recipient(caller: Principal, next: FeeRecipientConfig) -> Result<(), AdminError> {
    if next.owner == Principal::anonymous() || !matches!(next.subaccount.len(), 0 | 32) {
        return Err(AdminError::InvalidArgument("invalid fee recipient".into()));
    }
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let state = store
            .admin_state()
            .map_err(|_| AdminError::StorageFailure)?;
        let pending = store
            .pending_fee_payout_amount()
            .map_err(|_| AdminError::StorageFailure)?;
        let confirmation_relayer = store
            .config()
            .map_err(|_| AdminError::StorageFailure)?
            .map(|config| config.confirmation_relayer_principal)
            .ok_or(AdminError::StorageFailure)?;
        match bridge_core::fee_recipient_rotation_decision(
            authorized(&state, caller, ACTION_ROTATE),
            next.owner == Principal::anonymous(),
            next.owner == state.governance_principal
                || next.owner == state.pause_principal
                || next.owner == confirmation_relayer,
            next.subaccount.len(),
            pending,
        ) {
            bridge_core::FeeRecipientRotationDecision::Allow => {}
            bridge_core::FeeRecipientRotationDecision::Unauthorized => {
                return Err(AdminError::Unauthorized);
            }
            bridge_core::FeeRecipientRotationDecision::InvalidInput => {
                return Err(AdminError::InvalidArgument(
                    "fee recipient must not overlap governance, pause principal, or confirmation relayer".into(),
                ));
            }
            bridge_core::FeeRecipientRotationDecision::Busy => {
                return Err(AdminError::Busy);
            }
        }
        if next == state.fee_recipient {
            return Ok(());
        }
        store
            .rotate_fee_recipient_with_audit(
                next.clone(),
                caller,
                ic_cdk::api::time(),
                fee_recipient_digest(&state.fee_recipient).to_vec(),
                fee_recipient_digest(&next).to_vec(),
            )
            .map_err(|_| AdminError::StorageFailure)
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

pub fn request_fee_payout(caller: Principal, amount: Nat) -> Result<FeePayoutReceipt, AdminError> {
    STORE.with(|store| {
        let store = store.borrow();
        let admin = store
            .admin_state()
            .map_err(|_| AdminError::StorageFailure)?;
        if !authorized(&admin, caller, ACTION_PAYOUT) {
            return Err(AdminError::Unauthorized);
        }
        Ok(())
    })?;
    let amount = crate::api::bounded_nat_u128(&amount)
        .ok_or_else(|| AdminError::InvalidArgument("amount exceeds u128".into()))?;
    if amount == 0 {
        return Err(AdminError::InvalidArgument("amount must be nonzero".into()));
    }
    let fee = ledger::KINIC_LEDGER_FEE;
    let record = STORE.with(|store| {
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
        let payout = bridge_core::payout_decision(reserve, reserved, amount, fee.get(), true)
            .ok_or(AdminError::InsufficientFeeReserve)?;
        let payout_amount = payout
            .debit
            .checked_sub(fee.get())
            .ok_or(AdminError::InsufficientFeeReserve)?;
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
            amount: Amount::new(payout_amount),
            fee,
            from: Account::new(canister.as_slice().to_vec(), [0; 32])
                .map_err(|_| AdminError::StorageFailure)?,
            to: Account::new(admin.fee_recipient.owner.as_slice().to_vec(), subaccount)
                .map_err(|_| AdminError::InvalidArgument("invalid recipient".into()))?,
            spender: None,
        };
        let record = FeePayoutRecord {
            id,
            amount: payout_amount,
            recipient: admin.fee_recipient,
            transfer,
            state: FeePayoutState::Pending,
        };
        store
            .commit_fee_payout_request(&record, caller, created_at_time_ns)
            .map_err(|_| AdminError::StorageFailure)?;
        Ok(record)
    })?;
    Ok(FeePayoutReceipt {
        id: record.id,
        amount: Nat::from(record.amount),
        state: record.state,
    })
}
