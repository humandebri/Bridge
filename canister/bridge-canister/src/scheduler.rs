#[cfg(target_arch = "wasm32")]
use crate::{
    storage::{ConfirmationSchedulerHealth, SettlementJob, SettlementJobClaim, SettlementJobKind},
    tasks::{self, SettlementActionError, SettlementActionResult, SettlementStopReason},
    ActionKey, InFlightGuard, STORE,
};
#[cfg(target_arch = "wasm32")]
use std::time::Duration;

const MINUTE_NS: u64 = 60 * 1_000_000_000;
#[cfg(target_arch = "wasm32")]
const LEASE_NS: u64 = 120 * 1_000_000_000;
#[cfg(target_arch = "wasm32")]
const BUSY_RETRY_NS: u64 = 60 * 1_000_000_000;
pub fn confirmation_delay_ns(kind: bridge_core::EvmOperationKind, check_index: u8) -> Option<u64> {
    let _ = kind;
    let minutes = [2, 3, 5].get(usize::from(check_index)).copied()?;
    Some(minutes * MINUTE_NS)
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static SETTLEMENT_TIMER: std::cell::RefCell<Option<ic_cdk_timers::TimerId>> = const { std::cell::RefCell::new(None) };
}

pub fn arm() {
    #[cfg(target_arch = "wasm32")]
    {
        let next = STORE.with(|store| store.borrow().next_settlement_wakeup_ns());
        let Ok(next) = next else {
            mark_fault("failed to read the next settlement job");
            return;
        };
        SETTLEMENT_TIMER.with(|slot| {
            if let Some(timer) = slot.borrow_mut().take() {
                ic_cdk_timers::clear_timer(timer)
            }
            let Some(next_run_at_ns) = next else {
                mark_readable();
                return;
            };
            let delay = next_run_at_ns.saturating_sub(ic_cdk::api::time());
            let timer = ic_cdk_timers::set_timer(Duration::from_nanos(delay), async {
                SETTLEMENT_TIMER.with(|slot| {
                    slot.borrow_mut().take();
                });
                dispatch_due().await;
                arm();
            });
            *slot.borrow_mut() = Some(timer);
            mark_readable();
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
    let key = match job.kind {
        SettlementJobKind::Deposit => ActionKey::Deposit(job.settlement_id),
        SettlementJobKind::Withdrawal => ActionKey::Withdrawal(job.settlement_id),
    };
    let Some(_guard) = InFlightGuard::acquire(key) else {
        finish(
            &job,
            Some(now.saturating_add(BUSY_RETRY_NS)),
            job.confirmation_checks,
            None,
        );
        return;
    };
    mark_healthy();
    let result = match job.kind {
        SettlementJobKind::Deposit => {
            let result = tasks::advance_deposit(job.settlement_id).await;
            if let Ok(value) = &result {
                if crate::persist_deposit_settlement_result(job.settlement_id, value).is_err() {
                    mark_fault("failed to persist automatic deposit progress");
                    return;
                }
            }
            result
        }
        SettlementJobKind::Withdrawal => {
            let result = tasks::advance_withdrawal(job.settlement_id).await;
            if let Ok(value) = &result {
                if crate::persist_withdrawal_settlement_result(job.settlement_id, value).is_err() {
                    mark_fault("failed to persist automatic withdrawal progress");
                    return;
                }
            }
            result
        }
    };
    match result {
        Ok(SettlementActionResult::WaitingForConfirmation { .. }) => {
            let checks = job.confirmation_checks.saturating_add(1);
            let kind = job.operation_id.and_then(|operation_id| {
                STORE.with(|store| {
                    store
                        .borrow()
                        .evm_operation(operation_id)
                        .ok()
                        .flatten()
                        .map(|operation| operation.kind)
                })
            });
            let next_delay = kind.and_then(|kind| confirmation_delay_ns(kind, checks));
            if next_delay.is_none() {
                let stopped = SettlementActionResult::Stopped {
                    state: "Submitted".into(),
                    reason: SettlementStopReason::ConfirmationCheckExhausted,
                };
                let persisted = match job.kind {
                    SettlementJobKind::Deposit => {
                        crate::persist_deposit_settlement_result(job.settlement_id, &stopped)
                    }
                    SettlementJobKind::Withdrawal => {
                        crate::persist_withdrawal_settlement_result(job.settlement_id, &stopped)
                    }
                };
                if persisted.is_err() {
                    mark_fault("failed to persist confirmation exhaustion");
                    return;
                }
                finish(
                    &job,
                    None,
                    checks,
                    Some("Base transaction did not reach the Safe head within 10 minutes"),
                );
            } else {
                finish(
                    &job,
                    Some(ic_cdk::api::time().saturating_add(next_delay.unwrap_or_default())),
                    checks,
                    None,
                );
            }
        }
        Ok(SettlementActionResult::Stopped { ref reason, .. }) => finish(
            &job,
            None,
            job.confirmation_checks,
            Some(&format!("{reason:?}")),
        ),
        Ok(SettlementActionResult::ReconciliationProgress { .. }) => finish(
            &job,
            None,
            job.confirmation_checks,
            Some("Reconciliation requires manual progress"),
        ),
        Ok(SettlementActionResult::Submitted { .. }) => { /* submission atomically rescheduled this job */
        }
        Ok(SettlementActionResult::Complete { .. }) => {
            finish(&job, None, job.confirmation_checks, None)
        }
        Err(SettlementActionError::Busy) => finish(
            &job,
            Some(ic_cdk::api::time().saturating_add(BUSY_RETRY_NS)),
            job.confirmation_checks,
            None,
        ),
        Err(error) => finish(
            &job,
            None,
            job.confirmation_checks,
            Some(&format!("{error:?}")),
        ),
    }
}

#[cfg(target_arch = "wasm32")]
fn finish(job: &SettlementJob, next: Option<u64>, checks: u8, error: Option<&str>) {
    if STORE
        .with(|store| {
            store
                .borrow_mut()
                .finish_settlement_job(job, next, checks, error)
        })
        .is_err()
    {
        mark_fault("failed to persist settlement job outcome");
    }
}

#[cfg(target_arch = "wasm32")]
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

#[cfg(target_arch = "wasm32")]
fn mark_readable() {
    let _ = STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut health = store.confirmation_scheduler_health()?;
        health.healthy = true;
        health.last_error = None;
        store.set_confirmation_scheduler_health(&health)
    });
}

#[cfg(target_arch = "wasm32")]
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

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::EvmOperationKind;

    #[test]
    fn safe_operations_are_checked_at_cumulative_2_5_10_minutes() {
        for kind in [
            EvmOperationKind::MintDeposit,
            EvmOperationKind::CancelRelease,
            EvmOperationKind::RefundWithdrawal,
            EvmOperationKind::AcknowledgeRelease,
        ] {
            let delays = (0..4)
                .map(|index| confirmation_delay_ns(kind, index).map(|ns| ns / MINUTE_NS))
                .collect::<Vec<_>>();
            assert_eq!(delays, vec![Some(2), Some(3), Some(5), None]);
        }
    }
}
