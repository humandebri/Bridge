use bridge_core::{Amount, LedgerCallOutcome, LedgerFailure, LedgerTransferIdentity};
use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk::call::Call;
use icrc_ledger_types::{
    icrc1::{
        account::Account,
        transfer::{Memo, TransferArg, TransferError},
    },
    icrc2::transfer_from::{TransferFromArgs, TransferFromError},
    icrc3::{
        blocks::GetBlocksRequest,
        transactions::{GetTransactionsResponse, Transaction, TransactionRange},
    },
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryResolution {
    Succeeded { block_index: u128 },
    Absent { watermark: u128 },
    Incomplete,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
struct IndexStatus {
    num_blocks_synced: Nat,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
struct GetAccountTransactionsArgs {
    account: Account,
    start: Option<Nat>,
    max_results: Nat,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
struct TransactionWithId {
    id: Nat,
    transaction: Transaction,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
struct IndexTransactions {
    balance: Nat,
    transactions: Vec<TransactionWithId>,
    oldest_tx_id: Option<Nat>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
struct IndexError {
    message: String,
}

fn nat_u128(value: &Nat) -> Option<u128> {
    value.0.to_string().parse().ok()
}

fn amount(value: &Nat) -> Option<Amount> {
    nat_u128(value).map(Amount::new)
}

pub async fn ledger_fee(ledger: Principal) -> Result<Amount, ()> {
    let response = Call::unbounded_wait(ledger, "icrc1_fee")
        .with_arg(())
        .await
        .map_err(|_| ())?;
    let value: Nat = response.candid().map_err(|_| ())?;
    amount(&value).ok_or(())
}

pub async fn pull(ledger: Principal, identity: &LedgerTransferIdentity) -> LedgerCallOutcome {
    let args = TransferFromArgs {
        spender_subaccount: Some(
            identity
                .spender
                .as_ref()
                .map(|a| a.subaccount())
                .unwrap_or([0; 32]),
        ),
        from: Account {
            owner: Principal::from_slice(identity.from.owner()),
            subaccount: Some(identity.from.subaccount()),
        },
        to: Account {
            owner: Principal::from_slice(identity.to.owner()),
            subaccount: Some(identity.to.subaccount()),
        },
        amount: Nat::from(identity.amount.get()),
        fee: Some(Nat::from(identity.fee.get())),
        memo: Some(Memo::from(identity.memo.to_vec())),
        created_at_time: Some(identity.created_at_time_ns),
    };
    let response = match Call::unbounded_wait(ledger, "icrc2_transfer_from")
        .with_arg(&args)
        .await
    {
        Ok(response) => response,
        Err(_) => return LedgerCallOutcome::Ambiguous,
    };
    let result: Result<Nat, TransferFromError> = match response.candid() {
        Ok(result) => result,
        Err(_) => return LedgerCallOutcome::Ambiguous,
    };
    classify_transfer_from(result)
}

pub async fn release(ledger: Principal, identity: &LedgerTransferIdentity) -> LedgerCallOutcome {
    let args = TransferArg {
        from_subaccount: Some(identity.from.subaccount()),
        to: Account {
            owner: Principal::from_slice(identity.to.owner()),
            subaccount: Some(identity.to.subaccount()),
        },
        amount: Nat::from(identity.amount.get()),
        fee: Some(Nat::from(identity.fee.get())),
        memo: Some(Memo::from(identity.memo.to_vec())),
        created_at_time: Some(identity.created_at_time_ns),
    };
    let response = match Call::unbounded_wait(ledger, "icrc1_transfer")
        .with_arg(&args)
        .await
    {
        Ok(response) => response,
        Err(_) => return LedgerCallOutcome::Ambiguous,
    };
    let result: Result<Nat, TransferError> = match response.candid() {
        Ok(result) => result,
        Err(_) => return LedgerCallOutcome::Ambiguous,
    };
    classify_transfer(result)
}

pub async fn reconcile_history(
    ledger: Principal,
    identity: &LedgerTransferIdentity,
) -> HistoryResolution {
    const PAGE: u128 = 1_000;
    let mut cursor = 0u128;
    loop {
        let request = GetBlocksRequest {
            start: Nat::from(cursor),
            length: Nat::from(PAGE),
        };
        let response = match Call::unbounded_wait(ledger, "get_transactions")
            .with_arg(&request)
            .await
            .ok()
            .and_then(|response| response.candid::<GetTransactionsResponse>().ok())
        {
            Some(response) => response,
            None => return HistoryResolution::Incomplete,
        };
        let Some(log_length) = nat_u128(&response.log_length) else {
            return HistoryResolution::Incomplete;
        };
        let Some(first_index) = nat_u128(&response.first_index) else {
            return HistoryResolution::Incomplete;
        };
        for (offset, transaction) in response.transactions.iter().enumerate() {
            if matches_identity(transaction, identity) {
                return HistoryResolution::Succeeded {
                    block_index: first_index + offset as u128,
                };
            }
        }
        let requested_end = log_length.min(cursor.saturating_add(PAGE));
        let mut covered = response.transactions.len() as u128;
        for archived in response.archived_transactions {
            let (Some(start), Some(length)) =
                (nat_u128(&archived.start), nat_u128(&archived.length))
            else {
                return HistoryResolution::Incomplete;
            };
            let request = GetBlocksRequest {
                start: Nat::from(start),
                length: Nat::from(length),
            };
            let range = match Call::unbounded_wait(
                archived.callback.canister_id,
                &archived.callback.method,
            )
            .with_arg(&request)
            .await
            .ok()
            .and_then(|response| response.candid::<TransactionRange>().ok())
            {
                Some(range) if range.transactions.len() as u128 == length => range,
                _ => return HistoryResolution::Incomplete,
            };
            for (offset, transaction) in range.transactions.iter().enumerate() {
                if matches_identity(transaction, identity) {
                    return HistoryResolution::Succeeded {
                        block_index: start + offset as u128,
                    };
                }
            }
            covered = match covered.checked_add(length) {
                Some(value) => value,
                None => return HistoryResolution::Incomplete,
            };
        }
        if covered != requested_end.saturating_sub(cursor) {
            return HistoryResolution::Incomplete;
        }
        cursor = requested_end;
        if cursor == log_length {
            return HistoryResolution::Absent {
                watermark: log_length,
            };
        }
    }
}

pub async fn reconcile_index(
    index: Principal,
    expected_ledger: Principal,
    identity: &LedgerTransferIdentity,
    ledger_watermark: u128,
) -> HistoryResolution {
    let ledger_matches = match Call::unbounded_wait(index, "ledger_id")
        .with_arg(())
        .await
        .ok()
        .and_then(|response| response.candid::<Principal>().ok())
    {
        Some(id) => id == expected_ledger,
        None => false,
    };
    if !ledger_matches {
        return HistoryResolution::Incomplete;
    }
    let status = match Call::unbounded_wait(index, "status")
        .with_arg(())
        .await
        .ok()
        .and_then(|response| response.candid::<IndexStatus>().ok())
    {
        Some(status) => status,
        None => return HistoryResolution::Incomplete,
    };
    let Some(index_watermark) = nat_u128(&status.num_blocks_synced) else {
        return HistoryResolution::Incomplete;
    };
    if index_watermark < ledger_watermark {
        return HistoryResolution::Incomplete;
    }

    let account = Account {
        owner: Principal::from_slice(identity.from.owner()),
        subaccount: Some(identity.from.subaccount()),
    };
    let mut start = None;
    loop {
        let args = GetAccountTransactionsArgs {
            account,
            start: start.clone(),
            max_results: Nat::from(100u16),
        };
        let result = match Call::unbounded_wait(index, "get_account_transactions")
            .with_arg(&args)
            .await
            .ok()
            .and_then(|response| {
                response
                    .candid::<Result<IndexTransactions, IndexError>>()
                    .ok()
            }) {
            Some(Ok(result)) => result,
            _ => return HistoryResolution::Incomplete,
        };
        for transaction in &result.transactions {
            if matches_identity(&transaction.transaction, identity) {
                return nat_u128(&transaction.id)
                    .map(|block_index| HistoryResolution::Succeeded { block_index })
                    .unwrap_or(HistoryResolution::Incomplete);
            }
        }
        let Some(last) = result.transactions.last().and_then(|tx| nat_u128(&tx.id)) else {
            return HistoryResolution::Absent {
                watermark: index_watermark,
            };
        };
        if result
            .oldest_tx_id
            .as_ref()
            .and_then(nat_u128)
            .is_some_and(|oldest| last <= oldest)
        {
            return HistoryResolution::Absent {
                watermark: index_watermark,
            };
        }
        start = Some(Nat::from(last));
    }
}

fn matches_identity(transaction: &Transaction, identity: &LedgerTransferIdentity) -> bool {
    let Some(transfer) = transaction.transfer.as_ref() else {
        return false;
    };
    nat_u128(&transfer.amount) == Some(identity.amount.get())
        && transfer.fee.as_ref().and_then(nat_u128) == Some(identity.fee.get())
        && transfer.created_at_time == Some(identity.created_at_time_ns)
        && transfer.memo.as_ref().map(|memo| memo.0.as_ref()) == Some(identity.memo.as_slice())
        && account_matches(&transfer.from, &identity.from)
        && account_matches(&transfer.to, &identity.to)
        && match (&transfer.spender, &identity.spender) {
            (None, None) => true,
            (Some(actual), Some(expected)) => account_matches(actual, expected),
            _ => false,
        }
}

fn account_matches(actual: &Account, expected: &bridge_core::Account) -> bool {
    actual.owner.as_slice() == expected.owner()
        && actual.effective_subaccount() == &expected.subaccount()
}

fn classify_transfer_from(result: Result<Nat, TransferFromError>) -> LedgerCallOutcome {
    match result {
        Ok(index) => block(index, false),
        Err(TransferFromError::Duplicate { duplicate_of }) => block(duplicate_of, true),
        Err(TransferFromError::BadFee { expected_fee }) => {
            failure_amount(expected_fee, |expected_fee| LedgerFailure::BadFee {
                expected_fee,
            })
        }
        Err(TransferFromError::BadBurn { min_burn_amount }) => {
            failure_amount(min_burn_amount, |minimum| LedgerFailure::BadBurn {
                minimum,
            })
        }
        Err(TransferFromError::InsufficientFunds { balance }) => {
            failure_amount(balance, |balance| LedgerFailure::InsufficientFunds {
                balance,
            })
        }
        Err(TransferFromError::InsufficientAllowance { allowance }) => {
            failure_amount(allowance, |allowance| {
                LedgerFailure::InsufficientAllowance { allowance }
            })
        }
        Err(TransferFromError::TooOld) => LedgerCallOutcome::Ambiguous,
        Err(TransferFromError::CreatedInFuture { ledger_time }) => {
            retryable(LedgerFailure::CreatedInFuture {
                ledger_time_ns: ledger_time,
            })
        }
        Err(TransferFromError::TemporarilyUnavailable) => {
            retryable(LedgerFailure::TemporarilyUnavailable)
        }
        Err(TransferFromError::GenericError { error_code, .. }) => nat_u128(&error_code)
            .and_then(|v| u64::try_from(v).ok())
            .map(|code| retryable(LedgerFailure::Generic { code }))
            .unwrap_or(LedgerCallOutcome::Ambiguous),
    }
}

fn classify_transfer(result: Result<Nat, TransferError>) -> LedgerCallOutcome {
    match result {
        Ok(index) => block(index, false),
        Err(TransferError::Duplicate { duplicate_of }) => block(duplicate_of, true),
        Err(TransferError::BadFee { expected_fee }) => {
            failure_amount(expected_fee, |expected_fee| LedgerFailure::BadFee {
                expected_fee,
            })
        }
        Err(TransferError::BadBurn { min_burn_amount }) => {
            failure_amount(min_burn_amount, |minimum| LedgerFailure::BadBurn {
                minimum,
            })
        }
        Err(TransferError::InsufficientFunds { balance }) => failure_amount(balance, |balance| {
            LedgerFailure::InsufficientFunds { balance }
        }),
        Err(TransferError::TooOld) => LedgerCallOutcome::Ambiguous,
        Err(TransferError::CreatedInFuture { ledger_time }) => {
            retryable(LedgerFailure::CreatedInFuture {
                ledger_time_ns: ledger_time,
            })
        }
        Err(TransferError::TemporarilyUnavailable) => {
            retryable(LedgerFailure::TemporarilyUnavailable)
        }
        Err(TransferError::GenericError { error_code, .. }) => nat_u128(&error_code)
            .and_then(|v| u64::try_from(v).ok())
            .map(|code| retryable(LedgerFailure::Generic { code }))
            .unwrap_or(LedgerCallOutcome::Ambiguous),
    }
}

fn block(index: Nat, duplicate: bool) -> LedgerCallOutcome {
    match nat_u128(&index) {
        Some(block_index) if duplicate => LedgerCallOutcome::Duplicate { block_index },
        Some(block_index) => LedgerCallOutcome::Succeeded { block_index },
        None => LedgerCallOutcome::Ambiguous,
    }
}

fn failure_amount(value: Nat, build: impl FnOnce(Amount) -> LedgerFailure) -> LedgerCallOutcome {
    amount(&value)
        .map(build)
        .map(definitive)
        .unwrap_or(LedgerCallOutcome::Ambiguous)
}

fn definitive(code: LedgerFailure) -> LedgerCallOutcome {
    LedgerCallOutcome::DefinitiveFailure { code }
}

fn retryable(code: LedgerFailure) -> LedgerCallOutcome {
    LedgerCallOutcome::RetryableFailure { code }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_is_a_confirmed_success_and_every_error_is_classified() {
        assert_eq!(
            classify_transfer_from(Err(TransferFromError::Duplicate {
                duplicate_of: Nat::from(7u8)
            }))
            .confirmed_block(),
            Some(7)
        );
        assert!(matches!(
            classify_transfer(Err(TransferError::BadFee {
                expected_fee: Nat::from(3u8)
            })),
            LedgerCallOutcome::DefinitiveFailure {
                code: LedgerFailure::BadFee { .. }
            }
        ));
        assert!(matches!(
            classify_transfer_from(Err(TransferFromError::TemporarilyUnavailable)),
            LedgerCallOutcome::RetryableFailure {
                code: LedgerFailure::TemporarilyUnavailable
            }
        ));
        assert!(LedgerCallOutcome::Ambiguous.requires_hold());
    }
}
