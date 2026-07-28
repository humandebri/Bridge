use super::{Deserialize, Serialize, StorageError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettlementAdmissionError {
    RateLimited { retry_after_seconds: u64 },
    Storage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettlementQuotaLimits {
    pub window_seconds: u64,
    pub global: u16,
    pub per_principal: u16,
    pub per_record: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmationSchedule {
    pub operation_id: u64,
    pub submitted_at_ns: u64,
    pub next_check_at_ns: u64,
    pub checks_completed: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementJobKind {
    Deposit,
    Withdrawal,
    FeePayout,
}

impl SettlementJobKind {
    pub(super) const fn sql(self) -> i64 {
        match self {
            Self::Deposit => 0,
            Self::Withdrawal => 1,
            Self::FeePayout => 2,
        }
    }
}

pub(crate) fn fee_payout_job_id(id: u64) -> [u8; 32] {
    let mut key = [0; 32];
    key[24..].copy_from_slice(&id.to_be_bytes());
    key
}

pub(crate) fn fee_payout_id_from_job(key: [u8; 32]) -> Result<u64, StorageError> {
    if key[..24] != [0; 24] {
        return Err(StorageError::DecodeFailed);
    }
    Ok(u64::from_be_bytes(
        key[24..]
            .try_into()
            .map_err(|_| StorageError::DecodeFailed)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_payout_job_keys_are_canonical_and_strictly_decoded() {
        let key = fee_payout_job_id(u64::MAX);
        assert_eq!(&key[..24], &[0; 24]);
        assert_eq!(fee_payout_id_from_job(key), Ok(u64::MAX));

        let mut noncanonical = key;
        noncanonical[0] = 1;
        assert_eq!(
            fee_payout_id_from_job(noncanonical),
            Err(StorageError::DecodeFailed)
        );
    }
}
