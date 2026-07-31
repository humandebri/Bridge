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

numeric_id!(GovernanceOperationId);
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
    RefundDeposit,
    ReleaseWithdrawal,
    FeePayout,
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
    pub finalized_head_block_number: u64,
    pub confirmed_block_timestamp: u64,
    pub service_fee: Amount,
    pub max_service_fee: Amount,
    pub per_deposit_limit: Amount,
    pub mint_window_limit: Amount,
    pub mint_window_started_at: u64,
    pub mint_window_duration: u64,
    pub minted_in_window: Amount,
}

impl BaseMintSnapshot {
    pub fn effective_minted_in_window(self) -> Amount {
        let expires_at = u128::from(self.mint_window_started_at)
            .saturating_add(u128::from(self.mint_window_duration));
        if u128::from(self.confirmed_block_timestamp) >= expires_at {
            Amount::ZERO
        } else {
            self.minted_in_window
        }
    }

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
        if self.effective_minted_in_window().checked_add(net_amount)? > self.mint_window_limit {
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
    pub fn net_service_fee(self) -> Result<Amount, CoreError> {
        crate::settlement_decision(
            self.amount_out.get(),
            self.ledger_fee.get(),
            self.service_fee.get(),
        )
        .map(|decision| Amount::new(decision.reserve_credit))
        .ok_or(CoreError::SettlementMismatch)
    }

    pub fn validate_committed(
        self,
        amount: Amount,
        max_service_fee: Amount,
    ) -> Result<(), CoreError> {
        if self.service_fee > max_service_fee {
            return Err(CoreError::ServiceFeeAboveMaximum);
        }
        if !crate::committed_quote_matches(
            amount.get(),
            self.amount_out.get(),
            self.service_fee.get(),
        ) {
            return Err(CoreError::SettlementMismatch);
        }
        if crate::settlement_decision(
            self.amount_out.get(),
            self.ledger_fee.get(),
            self.service_fee.get(),
        )
        .is_none()
        {
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
pub struct DepositAccountingEffects {
    pub reservation_after: Amount,
    pub reservation_add: Amount,
    pub reservation_release: Amount,
    pub fee_credit: Amount,
    pub pending_liability_debit: Amount,
    pub escrow_debit: Amount,
    pub mint_supply_increase: Amount,
}

impl DepositAccountingEffects {
    pub const ZERO: Self = Self {
        reservation_after: Amount::ZERO,
        reservation_add: Amount::ZERO,
        reservation_release: Amount::ZERO,
        fee_credit: Amount::ZERO,
        pending_liability_debit: Amount::ZERO,
        escrow_debit: Amount::ZERO,
        mint_supply_increase: Amount::ZERO,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApplyResult {
    pub outcome: ApplyOutcome,
    pub fee_delta: Amount,
    pub deposit_effects: Option<DepositAccountingEffects>,
}

impl ApplyResult {
    pub const fn applied(fee_delta: Amount) -> Self {
        Self {
            outcome: ApplyOutcome::Applied,
            fee_delta,
            deposit_effects: None,
        }
    }

    pub const fn applied_deposit(effects: DepositAccountingEffects) -> Self {
        Self {
            outcome: ApplyOutcome::Applied,
            fee_delta: effects.fee_credit,
            deposit_effects: Some(effects),
        }
    }

    pub const fn idempotent() -> Self {
        Self {
            outcome: ApplyOutcome::Idempotent,
            fee_delta: Amount::ZERO,
            deposit_effects: None,
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
    SettlementMismatch,
    StaleFinalizedObservation,
    ConflictingFinalizedObservation,
    InvalidLedgerOperation,
    InvalidTransition {
        entity: &'static str,
        event: &'static str,
    },
    ConflictingReplay,
    PayloadConflict,
    HoldMismatch,
    MissingReconciliationEvidence,
    AttemptOverflow,
    AttemptPayloadChanged,
    RefundIneligible,
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CoreError {}
