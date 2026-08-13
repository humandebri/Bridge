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

#[derive(Debug)]
pub struct PrepaidQuota {
    kind: SettlementJobKind,
    settlement_id: [u8; 32],
    caller: candid::Principal,
}

impl PrepaidQuota {
    pub(super) fn new(
        kind: SettlementJobKind,
        settlement_id: [u8; 32],
        caller: candid::Principal,
    ) -> Self {
        Self {
            kind,
            settlement_id,
            caller,
        }
    }

    pub(super) fn consume(
        self,
        kind: SettlementJobKind,
        settlement_id: [u8; 32],
        caller: candid::Principal,
    ) -> bool {
        self.kind == kind && self.settlement_id == settlement_id && self.caller == caller
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementJobKind {
    Deposit,
    Withdrawal,
    FeePayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementLeaseLane {
    Automatic,
    PublicManual,
    GovernanceRecovery,
}

impl SettlementLeaseLane {
    pub(super) const fn sql(self) -> i64 {
        match self {
            Self::Automatic => 0,
            Self::PublicManual => 1,
            Self::GovernanceRecovery => 2,
        }
    }

    pub(super) const fn capacity(self) -> u64 {
        match self {
            Self::Automatic | Self::PublicManual => 4,
            Self::GovernanceRecovery => 1,
        }
    }

    pub(super) fn from_sql(value: i64) -> Result<Self, StorageError> {
        match value {
            0 => Ok(Self::Automatic),
            1 => Ok(Self::PublicManual),
            2 => Ok(Self::GovernanceRecovery),
            _ => Err(StorageError::DecodeFailed),
        }
    }
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

pub(super) fn settlement_record_key(kind: SettlementJobKind, settlement_id: [u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(33);
    key.push(kind.sql() as u8);
    key.extend_from_slice(&settlement_id);
    key
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
