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
    ::bridge_core::kernel::administrator_authorized(
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
        match ::bridge_core::kernel::fee_recipient_rotation_decision(
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
        let payout =
            ::bridge_core::kernel::payout_decision(reserve, reserved, amount, fee.get(), true)
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

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FeeStatus {
    pub fee_reserve: Nat,
    pub pending_payout_debit: Nat,
    pub ledger_fee: Nat,
    pub max_payout_amount: Nat,
    pub fee_recipient: FeeRecipientConfig,
    pub next_fee_payout_id: u64,
}
#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FeePayoutView {
    pub id: u64,
    pub amount: Nat,
    pub recipient: FeeRecipientConfig,
    pub ledger_fee: Nat,
    pub state: FeePayoutState,
}
#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SnsFeePayoutProposal {
    pub payout_id: u64,
    pub amount: Nat,
    pub recipient: FeeRecipientConfig,
}

pub fn fee_status() -> Result<FeeStatus, AdminError> {
    STORE.with(|store| {
        let store = store.borrow();
        let reserve = store
            .accounting()
            .map_err(|_| AdminError::StorageFailure)?
            .fee_reserve
            .get();
        let pending = store
            .pending_fee_payout_amount()
            .map_err(|_| AdminError::StorageFailure)?;
        if pending > reserve {
            return Err(AdminError::StorageFailure);
        }
        let fee = ledger::KINIC_LEDGER_FEE.get();
        Ok(FeeStatus {
            fee_reserve: reserve.into(),
            pending_payout_debit: pending.into(),
            ledger_fee: fee.into(),
            max_payout_amount: ::bridge_core::kernel::fee_payout_capacity(reserve, pending, fee)
                .into(),
            fee_recipient: store
                .admin_state()
                .map_err(|_| AdminError::StorageFailure)?
                .fee_recipient,
            next_fee_payout_id: store
                .next_fee_payout_id()
                .map_err(|_| AdminError::StorageFailure)?,
        })
    })
}
pub fn fee_payout_view(id: u64) -> Result<Option<FeePayoutView>, AdminError> {
    STORE.with(|store| {
        store
            .borrow()
            .fee_payout(id)
            .map_err(|_| AdminError::StorageFailure)
            .map(|record| {
                record.map(|r| FeePayoutView {
                    id: r.id,
                    amount: r.amount.into(),
                    recipient: r.recipient,
                    ledger_fee: r.transfer.fee.get().into(),
                    state: r.state,
                })
            })
    })
}

pub(crate) fn check_sns_fee_identity(
    proposal: &SnsFeePayoutProposal,
    governance: bool,
    operational: bool,
    continuation: bool,
) -> Result<Option<FeePayoutView>, AdminError> {
    let amount = crate::api::bounded_nat_u128(&proposal.amount)
        .ok_or_else(|| AdminError::InvalidArgument("amount exceeds u128".into()))?;
    let existing = fee_payout_view(proposal.payout_id)?;
    let status = fee_status()?;
    let recipient_matches = existing
        .as_ref()
        .map_or(status.fee_recipient == proposal.recipient, |r| {
            r.recipient == proposal.recipient
        });
    let identity_matches = existing.as_ref().map_or(
        !continuation && proposal.payout_id == status.next_fee_payout_id,
        |r| r.amount == proposal.amount,
    );
    if !::bridge_core::kernel::sns_fee_payout_authorized(
        governance,
        operational,
        amount > 0,
        recipient_matches,
        identity_matches,
    ) {
        return Err(if !governance {
            AdminError::Unauthorized
        } else {
            AdminError::InvalidArgument(
                "proposal does not match current payout identity or lifecycle".into(),
            )
        });
    }
    if existing.is_none() && proposal.amount > status.max_payout_amount {
        return Err(AdminError::InsufficientFeeReserve);
    }
    if continuation
        && matches!(
            existing.as_ref().map(|r| &r.state),
            Some(FeePayoutState::Failed)
        )
    {
        return Err(AdminError::InvalidArgument(
            "payout has permanently failed".into(),
        ));
    }
    Ok(existing)
}

pub(crate) fn validate_sns_fee_payout(
    proposal: &SnsFeePayoutProposal,
    continuation: bool,
) -> Result<String, String> {
    let operational = crate::asset_operations_are_available().map_err(|e| format!("{e:?}"))?;
    let existing = check_sns_fee_identity(proposal, true, operational, continuation)
        .map_err(|e| format!("{e:?}"))?;
    let fee = existing.map_or(ledger::KINIC_LEDGER_FEE.get().into(), |r| r.ledger_fee);
    Ok(format!("{} KINIC fee payout {}: amount {} raw units, recipient {} subaccount {}, Ledger fee {} raw units. Acceptance or continuation is not proof of Ledger settlement.",
        if continuation { "Continue" } else { "Request" }, proposal.payout_id, proposal.amount, proposal.recipient.owner,
        proposal.recipient.subaccount.iter().map(|b| format!("{b:02x}")).collect::<String>(), fee))
}

#[cfg(test)]
mod fee_tests {
    use super::*;
    use crate::storage::StableStore;
    use bridge_core::AccountingState;
    use ic_sqlite_vfs::DefaultMemoryImpl;
    use serial_test::serial;

    #[test]
    #[serial]
    fn fee_queries_bind_reservations_replays_and_reopened_v36_state() {
        let memory = DefaultMemoryImpl::default();
        let mut store = StableStore::init(memory.clone()).unwrap();
        let mut admin = AdminState {
            deposits_paused: false,
            withdrawal_fee_guard: None,
            pause_principal: Principal::self_authenticating([6; 32]),
            governance_principal: Principal::self_authenticating([8; 32]),
            fee_recipient: FeeRecipientConfig {
                owner: Principal::self_authenticating([7; 32]),
                subaccount: vec![],
            },
        };
        admin.governance_principal = Principal::self_authenticating([8; 32]);
        admin.fee_recipient = FeeRecipientConfig {
            owner: Principal::self_authenticating([7; 32]),
            subaccount: vec![],
        };
        store.set_admin_state(&admin).unwrap();
        let fee = ledger::KINIC_LEDGER_FEE;
        store
            .set_accounting(&AccountingState {
                fee_reserve: Amount::new(fee.get() + 100),
                ..Default::default()
            })
            .unwrap();
        let proposal = SnsFeePayoutProposal {
            payout_id: store.next_fee_payout_id().unwrap(),
            amount: 100u128.into(),
            recipient: admin.fee_recipient.clone(),
        };
        STORE.with(|slot| slot.borrow_mut().0 = Some(store));
        assert_eq!(fee_status().unwrap().max_payout_amount, Nat::from(100u128));
        assert_eq!(
            check_sns_fee_identity(&proposal, false, true, false),
            Err(AdminError::Unauthorized)
        );
        assert!(check_sns_fee_identity(&proposal, true, false, false).is_err());
        assert_eq!(
            check_sns_fee_identity(&proposal, true, true, false),
            Ok(None)
        );
        let mut changed = proposal.clone();
        changed.amount = Nat::from(u128::MAX) + Nat::from(1u128);
        assert!(matches!(
            check_sns_fee_identity(&changed, true, true, false),
            Err(AdminError::InvalidArgument(_))
        ));
        changed.amount = Nat::from(0u128);
        assert!(check_sns_fee_identity(&changed, true, true, false).is_err());
        changed.amount = 101u128.into();
        assert_eq!(
            check_sns_fee_identity(&changed, true, true, false),
            Err(AdminError::InsufficientFeeReserve)
        );
        changed = proposal.clone();
        changed.recipient.owner = Principal::self_authenticating([9; 32]);
        assert!(check_sns_fee_identity(&changed, true, true, false).is_err());
        changed = proposal.clone();
        changed.payout_id += 1;
        assert!(check_sns_fee_identity(&changed, true, true, false).is_err());
        assert!(check_sns_fee_identity(&proposal, true, true, true).is_err());
        let record = FeePayoutRecord {
            id: proposal.payout_id,
            amount: 100,
            recipient: proposal.recipient.clone(),
            state: FeePayoutState::Pending,
            transfer: LedgerTransferIdentity {
                operation: LedgerOperation::FeePayout,
                created_at_time_ns: 1,
                memo: [1; 32],
                amount: Amount::new(100),
                fee,
                from: Account::new(admin.governance_principal.as_slice().to_vec(), [0; 32])
                    .unwrap(),
                to: Account::new(proposal.recipient.owner.as_slice().to_vec(), [0; 32]).unwrap(),
                spender: None,
            },
        };
        STORE
            .with(|slot| {
                slot.borrow_mut()
                    .commit_fee_payout_request(&record, admin.governance_principal, 1)
            })
            .unwrap();
        STORE
            .with(|slot| {
                slot.borrow_mut()
                    .set_accounting(&AccountingState::default())
            })
            .unwrap();
        assert_eq!(fee_status(), Err(AdminError::StorageFailure));
        STORE
            .with(|slot| {
                slot.borrow_mut().set_accounting(&AccountingState {
                    fee_reserve: Amount::new(fee.get() + 100),
                    ..Default::default()
                })
            })
            .unwrap();
        let before = fee_status().unwrap();
        assert_eq!(before.max_payout_amount, Nat::from(0u128));
        assert_eq!(before.pending_payout_debit, Nat::from(fee.get() + 100));
        for _ in 0..2 {
            assert!(check_sns_fee_identity(&proposal, true, true, false)
                .unwrap()
                .is_some());
        }
        assert_eq!(fee_status().unwrap(), before);
        changed = proposal.clone();
        changed.amount = 99u128.into();
        assert!(check_sns_fee_identity(&changed, true, true, false).is_err());
        let view = fee_payout_view(record.id).unwrap();
        STORE.with(|slot| slot.borrow_mut().0.take());
        let reopened = StableStore::reopen_after_upgrade(memory).unwrap();
        STORE.with(|slot| slot.borrow_mut().0 = Some(reopened));
        assert_eq!(fee_status().unwrap(), before);
        assert_eq!(fee_payout_view(record.id).unwrap(), view);
        STORE
            .with(|slot| slot.borrow_mut().complete_fee_payout_success(record.id, 42))
            .unwrap();
        assert_eq!(
            fee_payout_view(record.id).unwrap().unwrap().state,
            FeePayoutState::Succeeded { block_index: 42 }
        );
        assert!(check_sns_fee_identity(&proposal, true, true, true)
            .unwrap()
            .is_some());
        assert_eq!(fee_status().unwrap().fee_reserve, Nat::from(0u128));
        STORE.with(|slot| slot.borrow_mut().0.take());
    }
}
