#[cfg(target_arch = "wasm32")]
use crate::storage::SettlementJobClaim;
use crate::{
    storage::{ConfirmationSchedulerHealth, SettlementJob, SettlementJobKind},
    tasks::{self, SettlementActionError, SettlementActionResult},
    ActionKey, InFlightGuard, STORE,
};
#[cfg(target_arch = "wasm32")]
use std::time::Duration;

const LEASE_NS: u64 = 120 * 1_000_000_000;
const BUSY_RETRY_NS: u64 = 60 * 1_000_000_000;

pub(crate) struct SettlementLease {
    pub job: SettlementJob,
    expected_receipt_block_number: Option<u64>,
    expected_finalized_block_number: Option<u64>,
}

impl SettlementLease {
    pub(crate) fn new(job: SettlementJob) -> Self {
        Self {
            job,
            expected_receipt_block_number: None,
            expected_finalized_block_number: None,
        }
    }

    pub(crate) fn with_expected_confirmation(
        mut self,
        receipt_block_number: u64,
        finalized_block_number: u64,
    ) -> Self {
        self.expected_receipt_block_number = Some(receipt_block_number);
        self.expected_finalized_block_number = Some(finalized_block_number);
        self
    }

    pub(crate) fn expected_receipt_block_number(&self) -> Option<u64> {
        self.expected_receipt_block_number
    }

    pub(crate) fn expected_finalized_block_number(&self) -> Option<u64> {
        self.expected_finalized_block_number
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
    static BASE_GOVERNANCE_TIMER: std::cell::RefCell<Option<ic_cdk_timers::TimerId>> = const { std::cell::RefCell::new(None) };
}

pub fn arm_base_governance(caller: candid::Principal) {
    #[cfg(target_arch = "wasm32")]
    BASE_GOVERNANCE_TIMER.with(|slot| {
        if slot.borrow().is_some() {
            return;
        }
        let has_work = STORE.with(|store| {
            let store = store.borrow();
            store
                .governance_lane()
                .map(|(_, _, _, pending)| pending.is_some())
                .and_then(|pending| {
                    store
                        .next_emergency_base_action()
                        .map(|next| pending || next.is_some())
                })
                .unwrap_or(true)
        });
        if !has_work {
            return;
        }
        let timer = ic_cdk_timers::set_timer(Duration::from_secs(60), async move {
            BASE_GOVERNANCE_TIMER.with(|slot| {
                slot.borrow_mut().take();
            });
            let Some(_guard) = InFlightGuard::acquire(ActionKey::BaseGovernance) else {
                arm_base_governance(caller);
                return;
            };
            let effective_caller = STORE.with(|store| {
                store
                    .borrow()
                    .admin_state()
                    .map(|state| state.governance_principal)
                    .unwrap_or(caller)
            });
            let _ = crate::base_governance::process_emergency(effective_caller).await;
            arm_base_governance(caller);
        });
        *slot.borrow_mut() = Some(timer);
    });
    #[cfg(not(target_arch = "wasm32"))]
    let _ = caller;
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
        SETTLEMENT_TIMER.with(|slot| {
            if let Some(timer) = slot.borrow_mut().take() {
                ic_cdk_timers::clear_timer(timer)
            }
            let Some(next_run_at_ns) = next else {
                return;
            };
            let delay = next_run_at_ns.saturating_sub(now);
            let timer = ic_cdk_timers::set_timer(Duration::from_nanos(delay), async {
                SETTLEMENT_TIMER.with(|slot| {
                    slot.borrow_mut().take();
                });
                dispatch_due().await;
                arm();
            });
            *slot.borrow_mut() = Some(timer);
        });
    }
}

#[cfg(target_arch = "wasm32")]
async fn dispatch_due() {
    let now = ic_cdk::api::time();
    let claim = STORE.with(|store| {
        store
            .borrow_mut()
            .claim_due_settlement_job(now, now.saturating_add(LEASE_NS))
    });
    let job = match claim {
        Ok(SettlementJobClaim::Claimed(job)) => job,
        Ok(SettlementJobClaim::ActiveLease { .. } | SettlementJobClaim::None) => return,
        Err(_) => {
            mark_fault("failed to claim a due settlement job");
            return;
        }
    };
    let _ = run_claimed(job).await;
}

pub(crate) async fn run_claimed(
    job: SettlementJob,
) -> Result<SettlementActionResult, SettlementActionError> {
    run_claimed_inner(job, None).await
}

pub(crate) async fn run_claimed_confirmation(
    job: SettlementJob,
    expected_receipt_block_number: u64,
    expected_finalized_block_number: u64,
) -> Result<SettlementActionResult, SettlementActionError> {
    run_claimed_inner(
        job,
        Some((
            expected_receipt_block_number,
            expected_finalized_block_number,
        )),
    )
    .await
}

async fn run_claimed_inner(
    job: SettlementJob,
    expected_confirmation: Option<(u64, u64)>,
) -> Result<SettlementActionResult, SettlementActionError> {
    let now = ic_cdk::api::time();
    let wallet_confirmation = expected_confirmation.is_some();
    let mut lease = SettlementLease::new(job);
    if let Some((receipt_block_number, finalized_block_number)) = expected_confirmation {
        lease = lease.with_expected_confirmation(receipt_block_number, finalized_block_number);
    }
    // Keep a durable recovery wakeup armed before the first await. If this runner
    // traps, the leased SQLite job can still be reclaimed after its deadline.
    arm();
    let key = match lease.job.kind {
        SettlementJobKind::Deposit => ActionKey::Deposit(lease.job.settlement_id),
        SettlementJobKind::Withdrawal => ActionKey::Withdrawal(lease.job.settlement_id),
    };
    let Some(_guard) = InFlightGuard::acquire(key) else {
        finish(
            &lease.job,
            Some(now.saturating_add(BUSY_RETRY_NS)),
            lease.job.confirmation_checks,
            None,
            None,
        )?;
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
    };
    let record_stop_reason = result.as_ref().ok().and_then(tasks::stop_reason_text);
    let outcome = match &result {
        Ok(SettlementActionResult::WaitingForConfirmation { .. }) if wallet_confirmation => STORE
            .with(|store| {
                store
                    .borrow_mut()
                    .park_awaiting_confirmation(&lease.job, ic_cdk::api::time())
            })
            .map_err(|_| SettlementActionError::StorageFailure),
        Ok(SettlementActionResult::WaitingForConfirmation { .. }) => finish(
            &lease.job,
            Some(
                ic_cdk::api::time().saturating_add(
                    STORE
                        .with(|store| {
                            store
                                .borrow()
                                .config()
                                .ok()
                                .flatten()
                                .map(|config| config.evm_liveness.check_interval_seconds)
                                .unwrap_or(60)
                        })
                        .saturating_mul(1_000_000_000),
                ),
            ),
            lease.job.confirmation_checks.saturating_add(1),
            None,
            None,
        ),
        Ok(SettlementActionResult::Stopped { reason, .. }) => finish(
            &lease.job,
            None,
            lease.job.confirmation_checks,
            Some(("SettlementStopped", format!("{reason:?}"))),
            record_stop_reason.clone(),
        ),
        Ok(SettlementActionResult::ReconciliationProgress { .. }) => finish(
            &lease.job,
            None,
            lease.job.confirmation_checks,
            Some((
                "ReconciliationProgress",
                "Reconciliation requires manual progress".into(),
            )),
            record_stop_reason.clone(),
        ),
        Ok(SettlementActionResult::Submitted { .. }) => STORE
            .with(|store| {
                store
                    .borrow_mut()
                    .set_settlement_stop_reason_fenced(&lease.job, None)
            })
            .map(|_| ())
            .map_err(|_| SettlementActionError::StorageFailure),
        Ok(SettlementActionResult::Complete { .. }) => {
            finish(&lease.job, None, lease.job.confirmation_checks, None, None)
        }
        Err(SettlementActionError::Busy) => finish(
            &lease.job,
            Some(ic_cdk::api::time().saturating_add(BUSY_RETRY_NS)),
            lease.job.confirmation_checks,
            None,
            None,
        ),
        Err(error) => finish(
            &lease.job,
            None,
            lease.job.confirmation_checks,
            Some(("SettlementActionError", format!("{error:?}"))),
            Some(format!("{error:?}")),
        ),
    };
    if outcome.is_err() {
        mark_fault("failed to persist settlement job outcome");
        return Err(SettlementActionError::StorageFailure);
    }
    result
}

pub(crate) async fn run_newly_enqueued(
    kind: SettlementJobKind,
    settlement_id: [u8; 32],
) -> Option<SettlementActionResult> {
    let now = ic_cdk::api::time();
    let claim = STORE.with(|store| {
        store.borrow_mut().claim_specific_due_settlement_job(
            kind,
            settlement_id,
            now,
            now.saturating_add(LEASE_NS),
        )
    });
    match claim {
        Ok(crate::storage::SettlementJobClaim::Claimed(job)) => run_claimed(job).await.ok(),
        Ok(
            crate::storage::SettlementJobClaim::ActiveLease { .. }
            | crate::storage::SettlementJobClaim::None,
        ) => None,
        Err(_) => {
            mark_fault("failed to claim a newly enqueued settlement job");
            None
        }
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
    let health = ConfirmationSchedulerHealth {
        healthy: true,
        last_run_ns: ic_cdk::api::time(),
        last_error: None,
    };
    let _ = STORE.with(|store| {
        store
            .borrow_mut()
            .set_confirmation_scheduler_health(&health)
    });
}

fn mark_fault(message: &str) {
    let health = ConfirmationSchedulerHealth {
        healthy: false,
        last_run_ns: ic_cdk::api::time(),
        last_error: Some(message.into()),
    };
    let _ = STORE.with(|store| {
        store
            .borrow_mut()
            .set_confirmation_scheduler_health(&health)
    });
    ic_cdk::println!("settlement scheduler fault: {message}");
}
