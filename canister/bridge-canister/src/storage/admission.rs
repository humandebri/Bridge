use super::{
    DepositAdmissionControl, DepositCallerQuota, DepositFundingReservation, Principal, StorageError,
};
use bridge_core::ReservePolicy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepositQuotaAdmission {
    pub now_ns: u64,
    pub window_seconds: u64,
    pub global_limit: u16,
    pub per_principal_limit: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepositCycleAdmission {
    pub cycles_balance: u128,
    pub reserve_policy: ReservePolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepositAdmissionOutcome {
    Inserted,
    Existing,
}

pub(super) fn consume_deposit_quota(
    admission: &mut DepositAdmissionControl,
    owner: Principal,
    quota: DepositQuotaAdmission,
) -> Result<(), StorageError> {
    let window_ns = quota.window_seconds.saturating_mul(1_000_000_000);
    let window_id = quota.now_ns / window_ns;
    if admission.window_id != window_id {
        admission.window_id = window_id;
        admission.global_count = 0;
        admission.caller_counts.clear();
    }
    let retry_after_seconds = ((window_id + 1)
        .saturating_mul(window_ns)
        .saturating_sub(quota.now_ns)
        .saturating_add(999_999_999)
        / 1_000_000_000)
        .max(1);
    let caller_count = admission
        .caller_counts
        .iter()
        .find(|entry| entry.caller == owner.as_slice())
        .map_or(0, |entry| entry.count);
    if admission.global_count >= quota.global_limit || caller_count >= quota.per_principal_limit {
        return Err(StorageError::DepositRateLimited {
            retry_after_seconds,
        });
    }
    admission.global_count = admission
        .global_count
        .checked_add(1)
        .ok_or(StorageError::CounterOverflow)?;
    match admission
        .caller_counts
        .iter_mut()
        .find(|entry| entry.caller == owner.as_slice())
    {
        Some(entry) => {
            entry.count = entry
                .count
                .checked_add(1)
                .ok_or(StorageError::CounterOverflow)?
        }
        None => admission.caller_counts.push(DepositCallerQuota {
            caller: owner.as_slice().to_vec(),
            count: 1,
        }),
    }
    Ok(())
}

pub(super) fn consume_deposit_quota_and_reserve_funding(
    admission: &mut DepositAdmissionControl,
    owner: Principal,
    deposit_id: [u8; 32],
    quota: DepositQuotaAdmission,
) -> Result<(), StorageError> {
    if admission
        .funding_reservations
        .iter()
        .any(|reservation| reservation.deposit_id == deposit_id)
    {
        return Ok(());
    }
    let active_global = u16::try_from(admission.funding_reservations.len())
        .map_err(|_| StorageError::CounterOverflow)?;
    let active_caller = u16::try_from(
        admission
            .funding_reservations
            .iter()
            .filter(|entry| entry.caller == owner.as_slice())
            .count(),
    )
    .map_err(|_| StorageError::CounterOverflow)?;
    if active_global >= 16 || active_caller >= 1 {
        return Err(StorageError::DepositRateLimited {
            retry_after_seconds: 1,
        });
    }
    consume_deposit_quota(admission, owner, quota)?;
    admission
        .funding_reservations
        .push(DepositFundingReservation {
            deposit_id,
            caller: owner.as_slice().to_vec(),
        });
    Ok(())
}

pub(super) fn release_deposit_funding_reservation(
    admission: &mut DepositAdmissionControl,
    owner: Principal,
    deposit_id: [u8; 32],
) -> Result<(), StorageError> {
    let Some(index) = admission
        .funding_reservations
        .iter()
        .position(|reservation| {
            reservation.deposit_id == deposit_id && reservation.caller == owner.as_slice()
        })
    else {
        return Err(StorageError::RecordNotFound);
    };
    admission.funding_reservations.remove(index);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deposit_quota_consumption_resets_windows_and_enforces_both_limits() {
        let owner = Principal::anonymous();
        let mut admission = DepositAdmissionControl::default();
        let quota = DepositQuotaAdmission {
            now_ns: 1,
            window_seconds: 60,
            global_limit: 2,
            per_principal_limit: 1,
        };
        consume_deposit_quota(&mut admission, owner, quota).expect("first admission");
        assert!(matches!(
            consume_deposit_quota(&mut admission, owner, quota),
            Err(StorageError::DepositRateLimited { .. })
        ));

        let next_window = DepositQuotaAdmission {
            now_ns: 60_000_000_001,
            ..quota
        };
        consume_deposit_quota(&mut admission, owner, next_window).expect("reset window");
        assert_eq!(admission.global_count, 1);
        assert_eq!(admission.caller_counts.len(), 1);
    }
}
