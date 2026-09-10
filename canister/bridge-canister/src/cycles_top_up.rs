use candid::{CandidType, Deserialize, Principal};
use ic_cdk::call::Call;
use std::cell::Cell;
#[cfg(target_arch = "wasm32")]
use std::time::Duration;

const LAUNCHER_PRINCIPAL: &str = "xfug4-5qaaa-aaaak-afowa-cai";
const THRESHOLD_CYCLES: u128 = 2_000_000_000_000;
#[cfg(target_arch = "wasm32")]
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

thread_local! {
    static IN_PROGRESS: Cell<bool> = const { Cell::new(false) };
}

#[derive(Debug, CandidType, Deserialize)]
enum LauncherError {
    Unauthorized,
    TooSoon,
    LauncherBalanceTooLow,
    TopUpFailed(String),
}

#[derive(Debug, CandidType, Deserialize)]
enum LauncherResult {
    Ok,
    Err(LauncherError),
}

struct RequestGuard;

impl RequestGuard {
    fn acquire(balance: u128, authorized: bool) -> Option<Self> {
        IN_PROGRESS.with(|flag| {
            if !::bridge_core::kernel::cycles_top_up_request_allowed(
                balance,
                THRESHOLD_CYCLES,
                flag.get(),
                authorized,
            ) {
                return None;
            }
            flag.set(true);
            Some(Self)
        })
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        IN_PROGRESS.with(|flag| flag.set(false));
    }
}

pub fn start() {
    #[cfg(target_arch = "wasm32")]
    {
        ic_cdk_timers::set_timer(Duration::ZERO, async {
            let _ = check(true).await;
        });
        ic_cdk_timers::set_timer_interval(CHECK_INTERVAL, || async {
            let _ = check(true).await;
        });
    }
}

// Only lifecycle timers use `true`; the manual endpoint supplies live controller status.
pub async fn check(authorized: bool) -> Result<(), String> {
    if !authorized {
        return Err("controller only".to_string());
    }
    let Some(_guard) = RequestGuard::acquire(ic_cdk::api::canister_cycle_balance(), authorized)
    else {
        return Ok(());
    };
    let result = request_cycles().await;
    if let Err(error) = &result {
        ic_cdk::println!("cycles top-up failed: {error}");
    }
    result
}

async fn request_cycles() -> Result<(), String> {
    let launcher = Principal::from_text(LAUNCHER_PRINCIPAL)
        .map_err(|error| format!("invalid launcher principal: {error}"))?;
    // A bounded-wait error can leave the remote outcome unknown. Do not retry here.
    let response = Call::bounded_wait(launcher, "request_cycles")
        .await
        .map_err(|error| {
            format!("request_cycles call failed (outcome may be unknown): {error:?}")
        })?;
    decode_response(response.as_ref())
}

fn decode_response(bytes: &[u8]) -> Result<(), String> {
    let result: LauncherResult = candid::decode_one(bytes)
        .map_err(|error| format!("request_cycles decode failed: {error}"))?;
    match result {
        LauncherResult::Ok => Ok(()),
        LauncherResult::Err(error) => Err(format!("launcher rejected: {error:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_top_up_request_policy_boundaries_and_authority() {
        for balance in [
            0,
            THRESHOLD_CYCLES - 1,
            THRESHOLD_CYCLES,
            THRESHOLD_CYCLES + 1,
            u128::MAX,
        ] {
            for authorized in [false, true] {
                for busy in [false, true] {
                    assert_eq!(
                        ::bridge_core::kernel::cycles_top_up_request_allowed(
                            balance,
                            THRESHOLD_CYCLES,
                            busy,
                            authorized,
                        ),
                        authorized && !busy && balance <= THRESHOLD_CYCLES
                    );
                }
            }
        }
        assert_eq!(
            Principal::from_text(LAUNCHER_PRINCIPAL).unwrap().to_text(),
            LAUNCHER_PRINCIPAL
        );
    }

    #[test]
    fn cycles_top_up_guard_excludes_overlap_and_releases_on_drop() {
        assert!(RequestGuard::acquire(0, false).is_none());
        assert!(RequestGuard::acquire(THRESHOLD_CYCLES + 1, true).is_none());
        let guard = RequestGuard::acquire(THRESHOLD_CYCLES, true).unwrap();
        assert!(RequestGuard::acquire(0, true).is_none());
        drop(guard);
        assert!(RequestGuard::acquire(0, true).is_some());
        let _ = std::panic::catch_unwind(|| {
            let _guard = RequestGuard::acquire(0, true).unwrap();
            panic!("simulated unwind");
        });
        assert!(RequestGuard::acquire(0, true).is_some());
    }

    #[test]
    fn cycles_top_up_decodes_launcher_success_rejections_and_invalid_responses() {
        assert_eq!(
            decode_response(&candid::encode_one(LauncherResult::Ok).unwrap()),
            Ok(())
        );
        for (error, expected) in [
            (LauncherError::Unauthorized, "Unauthorized"),
            (LauncherError::TooSoon, "TooSoon"),
            (
                LauncherError::LauncherBalanceTooLow,
                "LauncherBalanceTooLow",
            ),
            (
                LauncherError::TopUpFailed("deposit rejected".into()),
                "deposit rejected",
            ),
        ] {
            assert!(
                decode_response(&candid::encode_one(LauncherResult::Err(error)).unwrap())
                    .unwrap_err()
                    .contains(expected)
            );
        }
        for bytes in [
            vec![],
            b"invalid".to_vec(),
            candid::encode_one(42_u64).unwrap(),
        ] {
            assert!(decode_response(&bytes)
                .unwrap_err()
                .starts_with("request_cycles decode failed:"));
        }
    }
}
