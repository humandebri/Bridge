use candid::{CandidType, Decode, Deserialize, Encode, Principal};
use ic_cdk::call::Call;
use ic_stable_structures::{storable::Bound, DefaultMemoryImpl, StableCell, Storable};
use serde::Serialize;
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    time::Duration,
};

const MAX_STATE_BYTES: u32 = 2_048;
const NANOS_PER_SECOND: u64 = 1_000_000_000;

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct WatchdogInitArgs {
    pub bridge_canister: Principal,
    pub poll_interval_seconds: u64,
    pub stale_after_seconds: u64,
    pub failure_threshold: u8,
}

impl WatchdogInitArgs {
    fn validate(&self) -> Result<(), &'static str> {
        if self.bridge_canister == Principal::anonymous() {
            return Err("bridge canister must not be anonymous");
        }
        if self.poll_interval_seconds != 60
            || self.stale_after_seconds != 15 * 60
            || self.failure_threshold != 3
        {
            return Err("watchdog policy must be 60s polling, 15m staleness, and 3 failures");
        }
        Ok(())
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WatchdogStatus {
    pub consecutive_failures: u8,
    pub last_success_ns: u64,
    pub last_pause_attempt_ns: u64,
    pub pause_attempts: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Default)]
struct StableState {
    config: Option<WatchdogInitArgs>,
    status: WatchdogStatus,
    installed_at_ns: u64,
}

impl Storable for StableState {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(Encode!(self).expect("bounded watchdog state encoding"))
    }

    fn into_bytes(self) -> Vec<u8> {
        Encode!(&self).expect("bounded watchdog state encoding")
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).expect("valid watchdog stable state")
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: MAX_STATE_BYTES,
        is_fixed_size: false,
    };
}

thread_local! {
    static STATE: RefCell<StableCell<StableState, DefaultMemoryImpl>> = RefCell::new(
        StableCell::init(DefaultMemoryImpl::default(), StableState::default())
    );
    static TICK_RUNNING: Cell<bool> = const { Cell::new(false) };
}

struct TickGuard;

impl TickGuard {
    fn acquire() -> Option<Self> {
        TICK_RUNNING.with(|running| (!running.replace(true)).then_some(Self))
    }
}

impl Drop for TickGuard {
    fn drop(&mut self) {
        TICK_RUNNING.with(|running| running.set(false));
    }
}

#[derive(CandidType, Deserialize)]
struct ReserveStatus {
    sufficient: bool,
}

#[derive(CandidType, Deserialize)]
struct BridgeStatus {
    reserve: ReserveStatus,
    deposits_paused: bool,
    last_reserve_observation_ns: u64,
    last_finalized_observation_ns: u64,
}

#[derive(CandidType, Deserialize)]
enum AdminError {
    InsufficientFeeReserve,
    Unauthorized,
    InvalidArgument(String),
    StorageFailure,
}

fn observation_stale(observed_ns: u64, installed_ns: u64, now_ns: u64, stale_ns: u64) -> bool {
    let baseline = if observed_ns == 0 {
        installed_ns
    } else {
        observed_ns
    };
    now_ns.saturating_sub(baseline) > stale_ns
}

fn status_requires_pause(
    status: &BridgeStatus,
    installed_ns: u64,
    now_ns: u64,
    stale_ns: u64,
) -> bool {
    !status.reserve.sufficient
        || observation_stale(
            status.last_reserve_observation_ns,
            installed_ns,
            now_ns,
            stale_ns,
        )
        || observation_stale(
            status.last_finalized_observation_ns,
            installed_ns,
            now_ns,
            stale_ns,
        )
}

#[ic_cdk::init]
fn init(args: WatchdogInitArgs) {
    args.validate()
        .unwrap_or_else(|message| ic_cdk::trap(message));
    let now = ic_cdk::api::time();
    STATE.with(|cell| {
        cell.borrow_mut().set(StableState {
            config: Some(args.clone()),
            status: WatchdogStatus::default(),
            installed_at_ns: now,
        });
    });
    schedule(args.poll_interval_seconds);
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    let interval = STATE.with(|cell| {
        cell.borrow()
            .get()
            .config
            .as_ref()
            .map(|config| config.poll_interval_seconds)
    });
    schedule(interval.unwrap_or_else(|| ic_cdk::trap("missing watchdog configuration")));
}

fn schedule(seconds: u64) {
    ic_cdk_timers::set_timer_interval(Duration::from_secs(seconds), || async { tick().await });
}

async fn tick() {
    let Some(_guard) = TickGuard::acquire() else {
        return;
    };
    let Some((config, installed_at_ns)) = STATE.with(|cell| {
        let state = cell.borrow();
        Some((state.get().config.clone()?, state.get().installed_at_ns))
    }) else {
        return;
    };
    let response = Call::bounded_wait(config.bridge_canister, "get_bridge_status")
        .change_timeout(30)
        .await;
    let now = ic_cdk::api::time();
    let mut should_pause = false;
    STATE.with(|cell| {
        let mut cell = cell.borrow_mut();
        let mut state = cell.get().clone();
        let decoded = match response {
            Ok(value) => value.candid::<BridgeStatus>().ok(),
            Err(_) => None,
        };
        match decoded {
            Some(status) => {
                state.status.consecutive_failures = 0;
                state.status.last_success_ns = now;
                should_pause = !status.deposits_paused
                    && status_requires_pause(
                        &status,
                        installed_at_ns,
                        now,
                        config.stale_after_seconds.saturating_mul(NANOS_PER_SECOND),
                    );
            }
            None => {
                state.status.consecutive_failures =
                    state.status.consecutive_failures.saturating_add(1);
                should_pause = state.status.consecutive_failures >= config.failure_threshold;
            }
        }
        cell.set(state);
    });
    if !should_pause {
        return;
    }
    STATE.with(|cell| {
        let mut cell = cell.borrow_mut();
        let mut state = cell.get().clone();
        state.status.last_pause_attempt_ns = now;
        state.status.pause_attempts = state.status.pause_attempts.saturating_add(1);
        cell.set(state);
    });
    let response = Call::bounded_wait(config.bridge_canister, "pause_new_deposits")
        .change_timeout(30)
        .await;
    if let Ok(value) = response {
        let _result = value.candid::<Result<(), AdminError>>();
    }
}

#[ic_cdk::query]
fn get_watchdog_status() -> WatchdogStatus {
    STATE.with(|cell| cell.borrow().get().status)
}

ic_cdk::export_candid!();

#[cfg(test)]
mod tests {
    use super::*;

    fn status(reserve: bool, reserve_ns: u64, finalized_ns: u64) -> BridgeStatus {
        BridgeStatus {
            reserve: ReserveStatus {
                sufficient: reserve,
            },
            deposits_paused: false,
            last_reserve_observation_ns: reserve_ns,
            last_finalized_observation_ns: finalized_ns,
        }
    }

    #[test]
    fn pause_policy_observes_reserve_staleness_and_install_grace() {
        let minute = 60 * NANOS_PER_SECOND;
        assert!(!status_requires_pause(
            &status(true, 0, 0),
            minute,
            16 * minute,
            15 * minute
        ));
        assert!(status_requires_pause(
            &status(true, 0, 0),
            minute,
            17 * minute,
            15 * minute
        ));
        assert!(status_requires_pause(
            &status(false, 16 * minute, 16 * minute),
            minute,
            16 * minute,
            15 * minute
        ));
        assert!(!status_requires_pause(
            &status(true, 16 * minute, 16 * minute),
            minute,
            16 * minute,
            15 * minute
        ));
    }

    #[test]
    fn checked_in_candid_matches_rust_interface() {
        let normalize = |value: &str| {
            value
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>()
        };
        assert_eq!(
            normalize(&super::__export_service()),
            normalize(include_str!("../watchdog.did"))
        );
    }
}
