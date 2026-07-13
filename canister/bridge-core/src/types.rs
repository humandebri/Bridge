use core::fmt;

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Amount(u128);

impl Amount {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u128 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, CoreError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(CoreError::ArithmeticOverflow)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, CoreError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(CoreError::ArithmeticUnderflow)
    }
}

macro_rules! byte_id {
    ($name:ident) => {
        #[cfg_attr(
            feature = "storage-serde",
            derive(serde::Serialize, serde::Deserialize)
        )]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name([u8; 32]);

        impl $name {
            pub const fn new(bytes: [u8; 32]) -> Self {
                Self(bytes)
            }

            pub const fn bytes(self) -> [u8; 32] {
                self.0
            }
        }
    };
}

byte_id!(DepositId);
byte_id!(WithdrawalId);

macro_rules! numeric_id {
    ($name:ident) => {
        #[cfg_attr(
            feature = "storage-serde",
            derive(serde::Serialize, serde::Deserialize)
        )]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

numeric_id!(EvmOperationId);
numeric_id!(HoldId);

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    owner: Vec<u8>,
    subaccount: [u8; 32],
}

impl Account {
    pub fn new(owner: Vec<u8>, subaccount: [u8; 32]) -> Result<Self, CoreError> {
        if owner.is_empty() || owner.len() > 29 || owner.as_slice() == [4] {
            return Err(CoreError::InvalidPrincipal);
        }
        Ok(Self { owner, subaccount })
    }

    pub fn owner(&self) -> &[u8] {
        &self.owner
    }

    pub const fn subaccount(&self) -> [u8; 32] {
        self.subaccount
    }
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerOperation {
    PullDeposit,
    ReleaseWithdrawal,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerTransferIdentity {
    pub operation: LedgerOperation,
    pub created_at_time_ns: u64,
    pub memo: [u8; 32],
    pub amount: Amount,
    pub fee: Amount,
    pub from: Account,
    pub to: Account,
    pub spender: Option<Account>,
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaseMintSnapshot {
    pub service_fee: Amount,
    pub max_service_fee: Amount,
    pub per_deposit_limit: Amount,
    pub mint_window_limit: Amount,
    pub minted_in_window: Amount,
}

impl BaseMintSnapshot {
    pub fn quote(self, gross_amount: Amount, user_max_fee: Amount) -> Result<Amount, CoreError> {
        if gross_amount == Amount::ZERO {
            return Err(CoreError::InvalidAmount);
        }
        if self.service_fee > self.max_service_fee {
            return Err(CoreError::ServiceFeeAboveMaximum);
        }
        if self.service_fee > user_max_fee {
            return Err(CoreError::ServiceFeeAboveUserMaximum);
        }
        let net_amount = gross_amount.checked_sub(self.service_fee)?;
        if net_amount == Amount::ZERO {
            return Err(CoreError::InvalidAmount);
        }
        if net_amount > self.per_deposit_limit {
            return Err(CoreError::PerDepositLimitExceeded);
        }
        if self.minted_in_window.checked_add(net_amount)? > self.mint_window_limit {
            return Err(CoreError::MintWindowLimitExceeded);
        }
        Ok(net_amount)
    }
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settlement {
    pub amount_out: Amount,
    pub service_fee: Amount,
    pub ledger_fee: Amount,
}

impl Settlement {
    pub fn validate(
        self,
        amount: Amount,
        min_amount_out: Amount,
        max_service_fee: Amount,
    ) -> Result<(), CoreError> {
        if self.service_fee > max_service_fee {
            return Err(CoreError::ServiceFeeAboveMaximum);
        }
        if self.amount_out < min_amount_out {
            return Err(CoreError::MinimumAmountNotMet);
        }
        let total = self
            .amount_out
            .checked_add(self.service_fee)?
            .checked_add(self.ledger_fee)?;
        if total != amount {
            return Err(CoreError::SettlementMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied,
    Idempotent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplyResult {
    pub outcome: ApplyOutcome,
    pub fee_delta: Amount,
}

impl ApplyResult {
    pub const fn applied(fee_delta: Amount) -> Self {
        Self {
            outcome: ApplyOutcome::Applied,
            fee_delta,
        }
    }

    pub const fn idempotent() -> Self {
        Self {
            outcome: ApplyOutcome::Idempotent,
            fee_delta: Amount::ZERO,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreError {
    InvalidAmount,
    InvalidPrincipal,
    ArithmeticOverflow,
    ArithmeticUnderflow,
    ServiceFeeAboveMaximum,
    ServiceFeeAboveUserMaximum,
    PerDepositLimitExceeded,
    MintWindowLimitExceeded,
    MinimumAmountNotMet,
    SettlementMismatch,
    InsufficientSettlementReserve,
    InvalidLedgerOperation,
    InvalidTransition {
        entity: &'static str,
        event: &'static str,
    },
    ConflictingReplay,
    PayloadConflict,
    HoldMismatch,
    MissingReconciliationEvidence,
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CoreError {}
