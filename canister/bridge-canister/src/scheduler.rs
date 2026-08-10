#[cfg(target_arch = "wasm32")]
use crate::storage::DepositFundingAttemptState;
#[cfg(target_arch = "wasm32")]
use crate::storage::SettlementJobClaim;
use crate::{
    storage::{SettlementJob, SettlementJobKind, SettlementSchedulerHealth},
    tasks::{self, SettlementActionError, SettlementActionResult},
    ActionKey, InFlightGuard, STORE,
};
#[cfg(target_arch = "wasm32")]
use std::time::Duration;

// A receipt observation can make four bounded 30-second RPC calls (chain id,
// finalized head, receipt, and canonical block). Leave ample callback overhead
// while still keeping crash recovery bounded.
const LEASE_NS: u64 = 5 * 60 * 1_000_000_000;
const BUSY_RETRY_NS: u64 = 60 * 1_000_000_000;
const MAX_TRANSIENT_RETRY_NS: u64 = 15 * 60 * 1_000_000_000;
#[cfg(target_arch = "wasm32")]
const MAX_AUTOMATIC_SETTLEMENTS: u64 = 4;
#[cfg(target_arch = "wasm32")]
const FUNDING_RECOVERY_INTERVAL_SECONDS: u64 = 30;
#[cfg(any(target_arch = "wasm32", test))]
const LEDGER_DEDUP_NS: u64 = 24 * 60 * 60 * 1_000_000_000;
#[cfg(any(target_arch = "wasm32", test))]
const LEDGER_DEDUP_EXPIRY_MARGIN_NS: u64 = 60 * 1_000_000_000;

#[cfg(any(target_arch = "wasm32", test))]
fn funding_dedup_expired(created_at_time_ns: u64, now_ns: u64) -> bool {
    now_ns > funding_dedup_expiry_boundary_ns(created_at_time_ns)
}

#[cfg(any(target_arch = "wasm32", test))]
fn funding_dedup_expiry_boundary_ns(created_at_time_ns: u64) -> u64 {
    created_at_time_ns
        .saturating_add(LEDGER_DEDUP_NS)
        .saturating_add(LEDGER_DEDUP_EXPIRY_MARGIN_NS)
}

#[cfg(any(target_arch = "wasm32", test))]
fn funding_final_scan_at_ns(created_at_time_ns: u64) -> u64 {
    funding_dedup_expiry_boundary_ns(created_at_time_ns).saturating_add(1)
}

#[cfg(any(target_arch = "wasm32", test))]
fn fresh_funding_reconciliation_progress(
    progress: &bridge_core::ReconciliationScanProgress,
) -> bridge_core::ReconciliationScanProgress {
    bridge_core::ReconciliationScanProgress::new(progress.target.clone(), progress.transfer.clone())
}

fn transient_retry_delay_ns(base_seconds: u64, attempts: u8) -> u64 {
    let multiplier = 1u64 << attempts.min(4);
    base_seconds
        .max(1)
        .saturating_mul(1_000_000_000)
        .saturating_mul(multiplier)
        .min(MAX_TRANSIENT_RETRY_NS)
}

fn transient_stop(reason: &tasks::SettlementStopReason) -> bool {
    matches!(
        reason,
        tasks::SettlementStopReason::LedgerUnavailable
            | tasks::SettlementStopReason::LedgerAmbiguous
            | tasks::SettlementStopReason::RpcUnavailable
            | tasks::SettlementStopReason::SigningUnavailable
    )
}

#[cfg(any(target_arch = "wasm32", test))]
fn automatically_dispatches(kind: SettlementJobKind) -> bool {
    kind != SettlementJobKind::Withdrawal
}

fn terminal_fee_payout_result(result: &tasks::FeePayoutActionResult) -> bool {
    matches!(
        result,
        tasks::FeePayoutActionResult::Complete {
            state: crate::admin::FeePayoutState::Succeeded { .. }
                | crate::admin::FeePayoutState::Failed,
        } | tasks::FeePayoutActionResult::Stopped {
            state: crate::admin::FeePayoutState::Failed,
            ..
        }
    )
}

fn transient_retry_at(base_seconds: u64, checks: u8) -> u64 {
    ic_cdk::api::time().saturating_add(transient_retry_delay_ns(base_seconds, checks))
}

fn settlement_retry_interval_seconds() -> Result<u64, SettlementActionError> {
    STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| SettlementActionError::StorageFailure)?
            .map(|config| config.settlement_retry_interval_seconds)
            .ok_or(SettlementActionError::StorageFailure)
    })
}

pub(crate) struct SettlementLease {
    pub job: SettlementJob,
}

impl SettlementLease {
    pub(crate) fn new(job: SettlementJob) -> Self {
        Self { job }
    }

    pub(crate) fn job(&self) -> &SettlementJob {
        &self.job
    }

    pub(crate) fn renew_before_external_call(&mut self) -> Result<(), SettlementActionError> {
        let now = ic_cdk::api::time();
        let renewed = STORE.with(|store| {
            store.borrow_mut().renew_settlement_lease(
                &mut self.job,
                now,
                now.saturating_add(LEASE_NS),
            )
        });
        match renewed {
            Ok(true) => Ok(()),
            Ok(false) => Err(SettlementActionError::Busy),
            Err(_) => Err(SettlementActionError::StorageFailure),
        }
    }

    pub(crate) fn ensure_current(&self) -> Result<(), SettlementActionError> {
        match STORE.with(|store| store.borrow().settlement_lease_is_current(&self.job)) {
            Ok(true) => Ok(()),
            Ok(false) => Err(SettlementActionError::Busy),
            Err(_) => Err(SettlementActionError::StorageFailure),
        }
    }
}
#[cfg(target_arch = "wasm32")]
thread_local! {
    static SETTLEMENT_TIMER: std::cell::RefCell<Option<ic_cdk_timers::TimerId>> = const { std::cell::RefCell::new(None) };
    static FUNDING_RECOVERY_TIMER: std::cell::RefCell<Option<ic_cdk_timers::TimerId>> = const { std::cell::RefCell::new(None) };
}

pub fn arm_funding_recovery() {
    #[cfg(target_arch = "wasm32")]
    FUNDING_RECOVERY_TIMER.with(|slot| {
        if slot.borrow().is_some()
            || !STORE.with(|store| store.borrow().has_deposit_funding_attempts())
        {
            return;
        }
        let timer = ic_cdk_timers::set_timer(
            Duration::from_secs(FUNDING_RECOVERY_INTERVAL_SECONDS),
            async {
                FUNDING_RECOVERY_TIMER.with(|slot| {
                    slot.borrow_mut().take();
                });
                recover_one_funding_attempt().await;
                arm_funding_recovery();
            },
        );
        *slot.borrow_mut() = Some(timer);
    });
}

#[cfg(target_arch = "wasm32")]
async fn recover_one_funding_attempt() {
    let now = ic_cdk::api::time();
    let attempt = STORE.with(|store| {
        store
            .borrow()
            .next_deposit_funding_attempt_for_recovery(now)
    });
    let Ok(Some(attempt)) = attempt else {
        return;
    };
    let Ok(owner) = candid::Principal::try_from_slice(&attempt.intent.caller) else {
        mark_fault("invalid funding-attempt owner");
        return;
    };
    match &attempt.state {
        DepositFundingAttemptState::Prepared | DepositFundingAttemptState::Retryable { .. } => {
            if STORE
                .with(|store| {
                    store
                        .borrow_mut()
                        .remove_deposit_funding_attempt(owner, &attempt)
                })
                .is_err()
            {
                mark_fault("failed to prune a funding attempt");
            }
            return;
        }
        DepositFundingAttemptState::Dispatched { .. } => {}
        DepositFundingAttemptState::Reconciling { .. } => {}
    }

    let current = match &attempt.state {
        DepositFundingAttemptState::Dispatched { .. } => {
            let mut next = attempt.clone();
            next.state = DepositFundingAttemptState::Reconciling {
                progress: Box::new(bridge_core::ReconciliationScanProgress::new(
                    bridge_core::ReconciliationTarget::FundingAttempt(bridge_core::DepositId::new(
                        attempt.intent.deposit_id,
                    )),
                    attempt.transfer.clone(),
                )),
                next_check_at_ns: now,
                final_absence_scan: false,
            };
            next.updated_at_ns = now;
            if STORE
                .with(|store| {
                    store
                        .borrow_mut()
                        .update_deposit_funding_attempt(&attempt, &next)
                })
                .is_err()
            {
                mark_fault("failed to start funding reconciliation");
                return;
            }
            next
        }
        DepositFundingAttemptState::Reconciling { .. } => attempt,
        DepositFundingAttemptState::Prepared | DepositFundingAttemptState::Retryable { .. } => {
            return
        }
    };
    let DepositFundingAttemptState::Reconciling {
        progress,
        final_absence_scan,
        ..
    } = &current.state
    else {
        return;
    };
    let config = STORE.with(|store| store.borrow().config()).ok().flatten();
    let Some(config) = config else {
        mark_fault("missing funding reconciliation config");
        return;
    };
    match crate::ledger::reconcile_step(
        config.ledger_canister_id,
        config.index_canister_id,
        progress.as_ref().clone(),
    )
    .await
    {
        crate::ledger::ReconciliationOutcome::Progress(progress) => {
            let mut next = current.clone();
            next.state = DepositFundingAttemptState::Reconciling {
                progress,
                next_check_at_ns: ic_cdk::api::time()
                    .saturating_add(FUNDING_RECOVERY_INTERVAL_SECONDS * 1_000_000_000),
                final_absence_scan: *final_absence_scan,
            };
            next.updated_at_ns = ic_cdk::api::time();
            if STORE
                .with(|store| {
                    store
                        .borrow_mut()
                        .update_deposit_funding_attempt(&current, &next)
                })
                .is_err()
            {
                mark_fault("failed to persist funding reconciliation progress");
            }
        }
        crate::ledger::ReconciliationOutcome::Succeeded { block_index } => {
            if crate::api::promote_funding_success(&current, block_index, &config).is_err() {
                mark_fault("failed to promote reconciled funding");
            } else {
                arm();
            }
        }
        crate::ledger::ReconciliationOutcome::Absent { .. } => {
            let now = ic_cdk::api::time();
            let dedup_expired = funding_dedup_expired(current.transfer.created_at_time_ns, now);
            match bridge_core::funding_reconciliation_decision(
                true,
                *final_absence_scan,
                dedup_expired,
            ) {
                bridge_core::FundingReconciliationDecision::Wait => {
                    mark_fault("complete funding absence was not classified");
                }
                bridge_core::FundingReconciliationDecision::RestartFresh => {
                    let mut next = current.clone();
                    let DepositFundingAttemptState::Reconciling { progress, .. } = &current.state
                    else {
                        return;
                    };
                    next.state = DepositFundingAttemptState::Reconciling {
                        progress: Box::new(fresh_funding_reconciliation_progress(progress)),
                        next_check_at_ns: funding_final_scan_at_ns(
                            current.transfer.created_at_time_ns,
                        )
                        .max(now),
                        final_absence_scan: true,
                    };
                    next.updated_at_ns = now;
                    if STORE
                        .with(|store| {
                            store
                                .borrow_mut()
                                .update_deposit_funding_attempt(&current, &next)
                        })
                        .is_err()
                    {
                        mark_fault("failed to restart funding absence reconciliation");
                    }
                }
                bridge_core::FundingReconciliationDecision::Release => {
                    if STORE
                        .with(|store| {
                            store
                                .borrow_mut()
                                .remove_deposit_funding_attempt(owner, &current)
                        })
                        .is_err()
                    {
                        mark_fault("failed to release absent funding attempt");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod funding_recovery_tests {
    use super::{
        fresh_funding_reconciliation_progress, funding_dedup_expired,
        funding_dedup_expiry_boundary_ns, funding_final_scan_at_ns, LEDGER_DEDUP_EXPIRY_MARGIN_NS,
        LEDGER_DEDUP_NS,
    };
    use bridge_core::{
        Account, Amount, DepositId, LedgerOperation, LedgerTransferIdentity,
        ReconciliationScanPhase, ReconciliationScanProgress, ReconciliationTarget,
    };

    #[test]
    fn early_absence_restarts_the_same_transfer_from_a_fresh_ledger_cursor() {
        let transfer = LedgerTransferIdentity {
            operation: LedgerOperation::PullDeposit,
            created_at_time_ns: 17,
            memo: [3; 32],
            amount: Amount::new(100),
            fee: Amount::new(1),
            from: Account::new(vec![1], [2; 32]).expect("from account"),
            to: Account::new(vec![3], [4; 32]).expect("to account"),
            spender: Some(Account::new(vec![3], [0; 32]).expect("spender account")),
        };
        let target = ReconciliationTarget::FundingAttempt(DepositId::new([5; 32]));
        let mut stale = ReconciliationScanProgress::new(target.clone(), transfer.clone());
        stale.phase = ReconciliationScanPhase::Index {
            ledger_watermark: 100,
            index_watermark: Some(100),
            next_start: Some(1),
        };

        let fresh = fresh_funding_reconciliation_progress(&stale);

        assert_eq!(fresh.target, target);
        assert_eq!(fresh.transfer, transfer);
        assert!(matches!(
            fresh.phase,
            ReconciliationScanPhase::Ledger {
                next_block: 0,
                ledger_tip: None,
                pending_page: None,
            }
        ));
        assert_eq!(LEDGER_DEDUP_NS, 24 * 60 * 60 * 1_000_000_000);
    }

    #[test]
    fn dedup_expiry_is_strict_and_the_final_scan_starts_after_the_boundary() {
        let created_at = 17;
        let boundary = created_at + LEDGER_DEDUP_NS + LEDGER_DEDUP_EXPIRY_MARGIN_NS;
        assert!(!funding_dedup_expired(created_at, boundary - 1));
        assert!(!funding_dedup_expired(created_at, boundary));
        assert!(funding_dedup_expired(created_at, boundary + 1));
        assert_eq!(funding_dedup_expiry_boundary_ns(created_at), boundary);
        assert_eq!(funding_final_scan_at_ns(created_at), boundary + 1);
    }
}

pub fn arm() {
    #[cfg(target_arch = "wasm32")]
    {
        let now = ic_cdk::api::time();
        let next = STORE.with(|store| store.borrow().next_settlement_wakeup_ns(now));
        let Ok(next) = next else {
            mark_fault("failed to read the next settlement job");
            return;
        };
        if let Some(next_run_at_ns) = next {
            arm_at(next_run_at_ns);
        } else {
            SETTLEMENT_TIMER.with(|slot| {
                if let Some(timer) = slot.borrow_mut().take() {
                    ic_cdk_timers::clear_timer(timer)
                }
            });
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn arm_at(next_run_at_ns: u64) {
    let now = ic_cdk::api::time();
    SETTLEMENT_TIMER.with(|slot| {
        if let Some(timer) = slot.borrow_mut().take() {
            ic_cdk_timers::clear_timer(timer)
        }
        let delay = next_run_at_ns.saturating_sub(now);
        let timer = ic_cdk_timers::set_timer(Duration::from_nanos(delay), async {
            SETTLEMENT_TIMER.with(|slot| {
                slot.borrow_mut().take();
            });
            if let Some(capacity_available_at) = dispatch_due().await {
                arm_at(capacity_available_at);
            } else {
                arm()
            }
        });
        *slot.borrow_mut() = Some(timer);
    });
}

#[cfg(target_arch = "wasm32")]
async fn dispatch_due() -> Option<u64> {
    let now = ic_cdk::api::time();
    let claim = STORE.with(|store| {
        store.borrow_mut().claim_due_settlement_job(
            now,
            now.saturating_add(LEASE_NS),
            MAX_AUTOMATIC_SETTLEMENTS,
        )
    });
    let job = match claim {
        Ok(SettlementJobClaim::Claimed(job)) => job,
        Ok(SettlementJobClaim::ActiveLease { lease_until_ns }) => return Some(lease_until_ns),
        Ok(SettlementJobClaim::None) => return None,
        Err(_) => {
            mark_fault("failed to claim a due settlement job");
            return None;
        }
    };
    if !automatically_dispatches(job.kind) {
        if finish(
            &job,
            None,
            job.attempts,
            Some((
                "ManualContinuationRequired",
                "withdrawals are advanced only by explicit frontend calls".into(),
            )),
            Some("Manual continuation required".into()),
        )
        .is_err()
        {
            mark_fault("failed to park an automatically scheduled withdrawal");
        }
        return None;
    }
    match job.kind {
        SettlementJobKind::FeePayout => {
            let _ = run_claimed_fee_payout(job).await;
        }
        SettlementJobKind::Deposit => {
            let _ = run_claimed(job).await;
        }
        SettlementJobKind::Withdrawal => unreachable!("manual-only jobs were parked"),
    }
    None
}

pub(crate) async fn run_claimed(
    job: SettlementJob,
) -> Result<SettlementActionResult, SettlementActionError> {
    run_claimed_inner(job).await
}

pub(crate) async fn run_claimed_fee_payout(
    job: SettlementJob,
) -> Result<tasks::FeePayoutActionResult, SettlementActionError> {
    let now = ic_cdk::api::time();
    let retry_interval_seconds = settlement_retry_interval_seconds()?;
    let payout_id = crate::storage::fee_payout_id_from_job(job.settlement_id)
        .map_err(|_| SettlementActionError::InvalidId)?;
    let mut lease = SettlementLease::new(job);
    arm();
    let Some(_guard) = InFlightGuard::acquire(ActionKey::FeePayout(payout_id)) else {
        finish(
            &lease.job,
            Some(now.saturating_add(BUSY_RETRY_NS)),
            lease.job.attempts,
            None,
            None,
        )?;
        return Err(SettlementActionError::Busy);
    };
    mark_healthy();
    let result = tasks::advance_fee_payout(payout_id, &mut lease).await;
    let outcome = match &result {
        Ok(result) if terminal_fee_payout_result(result) => {
            finish(&lease.job, None, lease.job.attempts, None, None)
        }
        Ok(tasks::FeePayoutActionResult::Complete { .. }) => finish(
            &lease.job,
            None,
            lease.job.attempts,
            Some((
                "InvalidFeePayoutCompletion",
                "nonterminal fee payout returned Complete".into(),
            )),
            None,
        ),
        Ok(tasks::FeePayoutActionResult::ReconciliationProgress { .. }) => finish(
            &lease.job,
            Some(transient_retry_at(
                retry_interval_seconds,
                lease.job.attempts,
            )),
            lease.job.attempts.saturating_add(1),
            None,
            None,
        ),
        Ok(tasks::FeePayoutActionResult::Stopped { reason, .. }) if transient_stop(reason) => {
            finish(
                &lease.job,
                Some(transient_retry_at(
                    retry_interval_seconds,
                    lease.job.attempts,
                )),
                lease.job.attempts.saturating_add(1),
                None,
                None,
            )
        }
        Ok(tasks::FeePayoutActionResult::Stopped { reason, .. }) => finish(
            &lease.job,
            None,
            lease.job.attempts,
            Some(("SettlementStopped", format!("{reason:?}"))),
            None,
        ),
        Err(SettlementActionError::Busy) => finish(
            &lease.job,
            Some(now.saturating_add(BUSY_RETRY_NS)),
            lease.job.attempts,
            None,
            None,
        ),
        Err(error) => finish(
            &lease.job,
            None,
            lease.job.attempts,
            Some(("SettlementActionError", format!("{error:?}"))),
            None,
        ),
    };
    if outcome.is_err() {
        mark_fault("failed to persist fee payout job outcome");
        return Err(SettlementActionError::StorageFailure);
    }
    result
}

async fn run_claimed_inner(
    job: SettlementJob,
) -> Result<SettlementActionResult, SettlementActionError> {
    let now = ic_cdk::api::time();
    let mut lease = SettlementLease::new(job);
    if lease.job.kind == SettlementJobKind::Deposit {
        // Deposit jobs remain automatic and recover expired leases through the timer.
        arm();
    }
    let key = match lease.job.kind {
        SettlementJobKind::Deposit => ActionKey::Deposit(lease.job.settlement_id),
        SettlementJobKind::Withdrawal => ActionKey::Withdrawal(lease.job.settlement_id),
        SettlementJobKind::FeePayout => return Err(SettlementActionError::WrongState),
    };
    let Some(_guard) = InFlightGuard::acquire(key) else {
        if lease.job.kind == SettlementJobKind::Withdrawal {
            finish(
                &lease.job,
                None,
                lease.job.attempts,
                Some(("ManualContinuationRequired", "withdrawal is busy".into())),
                Some("Manual continuation required".into()),
            )?;
        } else {
            finish(
                &lease.job,
                Some(now.saturating_add(BUSY_RETRY_NS)),
                lease.job.attempts,
                None,
                None,
            )?;
        }
        return Err(SettlementActionError::Busy);
    };
    mark_healthy();
    let result = match lease.job.kind {
        SettlementJobKind::Deposit => {
            tasks::advance_deposit(lease.job.settlement_id, &mut lease).await
        }
        SettlementJobKind::Withdrawal => {
            tasks::advance_withdrawal(lease.job.settlement_id, &mut lease).await
        }
        SettlementJobKind::FeePayout => return Err(SettlementActionError::WrongState),
    };
    let record_stop_reason = result.as_ref().ok().and_then(tasks::stop_reason_text);
    let outcome = if lease.job.kind == SettlementJobKind::Withdrawal {
        finish_manual_withdrawal(&lease.job, &result, record_stop_reason.clone())
    } else {
        let retry_interval_seconds = settlement_retry_interval_seconds()?;
        match &result {
            Ok(SettlementActionResult::Stopped { reason, .. }) if transient_stop(reason) => finish(
                &lease.job,
                Some(transient_retry_at(
                    retry_interval_seconds,
                    lease.job.attempts,
                )),
                lease.job.attempts.saturating_add(1),
                None,
                record_stop_reason.clone(),
            ),
            Ok(SettlementActionResult::Stopped { reason, .. }) => finish(
                &lease.job,
                None,
                lease.job.attempts,
                Some(("SettlementStopped", format!("{reason:?}"))),
                record_stop_reason.clone(),
            ),
            Ok(SettlementActionResult::ReconciliationProgress { .. }) => finish(
                &lease.job,
                Some(transient_retry_at(
                    retry_interval_seconds,
                    lease.job.attempts,
                )),
                lease.job.attempts.saturating_add(1),
                None,
                None,
            ),
            Ok(SettlementActionResult::Deferred { next_run_at_ns, .. }) => finish(
                &lease.job,
                Some(*next_run_at_ns),
                lease.job.attempts,
                None,
                None,
            ),
            Ok(SettlementActionResult::Complete { .. }) => {
                finish(&lease.job, None, lease.job.attempts, None, None)
            }
            Err(SettlementActionError::Busy) => finish(
                &lease.job,
                Some(ic_cdk::api::time().saturating_add(BUSY_RETRY_NS)),
                lease.job.attempts,
                None,
                None,
            ),
            Err(error) => finish(
                &lease.job,
                None,
                lease.job.attempts,
                Some(("SettlementActionError", format!("{error:?}"))),
                Some(format!("{error:?}")),
            ),
        }
    };
    if outcome.is_err() {
        mark_fault("failed to persist settlement job outcome");
        return Err(SettlementActionError::StorageFailure);
    }
    result
}

fn finish_manual_withdrawal(
    job: &SettlementJob,
    result: &Result<SettlementActionResult, SettlementActionError>,
    record_stop_reason: Option<String>,
) -> Result<(), SettlementActionError> {
    match result {
        Ok(SettlementActionResult::Complete { .. }) => finish(job, None, job.attempts, None, None),
        Ok(SettlementActionResult::Stopped { reason, .. }) => finish(
            job,
            None,
            job.attempts,
            Some(("SettlementStopped", format!("{reason:?}"))),
            record_stop_reason,
        ),
        Ok(SettlementActionResult::ReconciliationProgress { .. }) => finish(
            job,
            None,
            job.attempts,
            Some((
                "ManualContinuationRequired",
                "withdrawal reconciliation requires another explicit call".into(),
            )),
            Some("Manual continuation required".into()),
        ),
        Ok(SettlementActionResult::Deferred { .. }) => finish(
            job,
            None,
            job.attempts,
            Some((
                "ManualContinuationRequired",
                "withdrawal continuation was deferred".into(),
            )),
            Some("Manual continuation required".into()),
        ),
        Err(error) => finish(
            job,
            None,
            job.attempts,
            Some(("SettlementActionError", format!("{error:?}"))),
            Some(format!("{error:?}")),
        ),
    }
}

fn finish(
    job: &SettlementJob,
    next: Option<u64>,
    checks: u8,
    error: Option<(&str, String)>,
    record_stop_reason: Option<String>,
) -> Result<(), SettlementActionError> {
    let now = ic_cdk::api::time();
    STORE
        .with(|store| {
            store.borrow_mut().finish_settlement_job(
                job,
                next,
                checks,
                error
                    .as_ref()
                    .map(|(code, detail)| (*code, detail.as_str())),
                record_stop_reason,
                now,
            )
        })
        .map_err(|_| SettlementActionError::StorageFailure)
}

fn mark_healthy() {
    let health = SettlementSchedulerHealth {
        healthy: true,
        last_run_ns: ic_cdk::api::time(),
        last_error: None,
    };
    let _ = STORE.with(|store| store.borrow_mut().set_settlement_scheduler_health(&health));
}

fn mark_fault(message: &str) {
    let health = SettlementSchedulerHealth {
        healthy: false,
        last_run_ns: ic_cdk::api::time(),
        last_error: Some(message.into()),
    };
    let _ = STORE.with(|store| store.borrow_mut().set_settlement_scheduler_health(&health));
    ic_cdk::println!("settlement scheduler fault: {message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_backoff_is_exponential_and_bounded() {
        assert_eq!(transient_retry_delay_ns(60, 0), 60 * 1_000_000_000);
        assert_eq!(transient_retry_delay_ns(60, 3), 480 * 1_000_000_000);
        assert_eq!(
            transient_retry_delay_ns(60, u8::MAX),
            MAX_TRANSIENT_RETRY_NS
        );
    }

    #[test]
    fn withdrawals_are_not_automatically_dispatched() {
        assert!(!automatically_dispatches(SettlementJobKind::Withdrawal));
        assert!(automatically_dispatches(SettlementJobKind::Deposit));
        assert!(automatically_dispatches(SettlementJobKind::FeePayout));
    }

    #[test]
    fn only_recoverable_stop_reasons_are_rescheduled() {
        assert!(transient_stop(&tasks::SettlementStopReason::RpcUnavailable));
        assert!(transient_stop(
            &tasks::SettlementStopReason::SigningUnavailable
        ));
        assert!(!transient_stop(
            &tasks::SettlementStopReason::LedgerFeeExceedsServiceFee
        ));
        assert!(!transient_stop(
            &tasks::SettlementStopReason::RpcInconsistent
        ));
        assert!(!transient_stop(
            &tasks::SettlementStopReason::BaseStateMismatch
        ));
    }

    #[test]
    fn terminal_fee_payout_results_delete_the_job_without_changing_the_public_result() {
        use crate::admin::FeePayoutState;

        for result in [
            tasks::FeePayoutActionResult::Complete {
                state: FeePayoutState::Succeeded { block_index: 7 },
            },
            tasks::FeePayoutActionResult::Complete {
                state: FeePayoutState::Failed,
            },
            tasks::FeePayoutActionResult::Stopped {
                state: FeePayoutState::Failed,
                reason: tasks::SettlementStopReason::LedgerRejected("BadFee".into()),
            },
        ] {
            assert!(terminal_fee_payout_result(&result));
        }
        assert!(!terminal_fee_payout_result(
            &tasks::FeePayoutActionResult::Stopped {
                state: FeePayoutState::Pending,
                reason: tasks::SettlementStopReason::LedgerUnavailable,
            }
        ));
        assert!(!terminal_fee_payout_result(
            &tasks::FeePayoutActionResult::Stopped {
                state: FeePayoutState::ReconciliationHold,
                reason: tasks::SettlementStopReason::LedgerAmbiguous,
            }
        ));
    }
}
