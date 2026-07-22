use bridge_core::{
    Amount, LedgerCallOutcome, LedgerFailure, LedgerTransferIdentity, ReconciliationArchiveRange,
    ReconciliationLedgerPage, ReconciliationScanPhase, ReconciliationScanProgress,
};
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

const LEDGER_CALL_TIMEOUT_SECONDS: u32 = 15;
pub const KINIC_LEDGER_FEE: Amount = Amount::new(10_000);

fn ledger_call(canister: Principal, method: &'static str) -> Call<'static, 'static> {
    Call::bounded_wait(canister, method).change_timeout(LEDGER_CALL_TIMEOUT_SECONDS)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReconciliationOutcome {
    Progress(Box<ReconciliationScanProgress>),
    Succeeded {
        block_index: u128,
    },
    Absent {
        ledger_watermark: u128,
        index_watermark: u128,
    },
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
    let response = match ledger_call(ledger, "icrc2_transfer_from")
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
    let response = match ledger_call(ledger, "icrc1_transfer").with_arg(&args).await {
        Ok(response) => response,
        Err(_) => return LedgerCallOutcome::Ambiguous,
    };
    let result: Result<Nat, TransferError> = match response.candid() {
        Ok(result) => result,
        Err(_) => return LedgerCallOutcome::Ambiguous,
    };
    classify_transfer(result)
}

pub async fn reconcile_step(
    ledger: Principal,
    index: Principal,
    mut progress: ReconciliationScanProgress,
) -> ReconciliationOutcome {
    const CALL_BUDGET: u8 = 4;
    match progress.phase.clone() {
        ReconciliationScanPhase::Ledger {
            next_block,
            ledger_tip,
            pending_page,
        } => {
            reconcile_ledger(
                ledger,
                index,
                &mut progress,
                next_block,
                ledger_tip,
                pending_page.map(|page| *page),
                CALL_BUDGET,
            )
            .await
        }
        ReconciliationScanPhase::Index {
            ledger_watermark,
            index_watermark,
            next_start,
        } => {
            reconcile_index(
                index,
                ledger,
                &mut progress,
                ledger_watermark,
                index_watermark,
                next_start,
                CALL_BUDGET,
            )
            .await
        }
    }
}

async fn reconcile_ledger(
    ledger: Principal,
    index: Principal,
    progress: &mut ReconciliationScanProgress,
    mut next_block: u128,
    mut ledger_tip: Option<u128>,
    mut pending_page: Option<ReconciliationLedgerPage>,
    mut budget: u8,
) -> ReconciliationOutcome {
    const PAGE: u128 = 1_000;
    loop {
        if let Some(mut page) = pending_page.take() {
            while usize::from(page.next_archive) < page.archives.len() && budget > 0 {
                let archive = &page.archives[usize::from(page.next_archive)];
                let Ok(canister_id) = Principal::try_from_slice(&archive.canister_id) else {
                    return ledger_progress(progress, next_block, ledger_tip, Some(page));
                };
                let request = GetBlocksRequest {
                    start: Nat::from(archive.start),
                    length: Nat::from(archive.length),
                };
                budget -= 1;
                let range = match Call::bounded_wait(canister_id, &archive.method)
                    .change_timeout(LEDGER_CALL_TIMEOUT_SECONDS)
                    .with_arg(&request)
                    .await
                    .ok()
                    .and_then(|response| response.candid::<TransactionRange>().ok())
                {
                    Some(range) if range.transactions.len() as u128 == archive.length => range,
                    _ => return ledger_progress(progress, next_block, ledger_tip, Some(page)),
                };
                for (offset, transaction) in range.transactions.iter().enumerate() {
                    if matches_identity(transaction, &progress.transfer) {
                        return ReconciliationOutcome::Succeeded {
                            block_index: archive.start + offset as u128,
                        };
                    }
                }
                page.next_archive += 1;
            }
            if usize::from(page.next_archive) < page.archives.len() {
                return ledger_progress(progress, next_block, ledger_tip, Some(page));
            }
            next_block = page.end;
        }

        if ledger_tip.is_some_and(|exclusive_tip| {
            exclusive_tip == 0
                || bridge_core::scan_complete(
                    next_block,
                    exclusive_tip - 1,
                    exclusive_tip - 1,
                    true,
                    false,
                )
        }) {
            let ledger_watermark = ledger_tip.expect("checked ledger tip");
            progress.phase = ReconciliationScanPhase::Index {
                ledger_watermark,
                index_watermark: None,
                next_start: None,
            };
            return reconcile_index(
                index,
                ledger,
                progress,
                ledger_watermark,
                None,
                None,
                budget,
            )
            .await;
        }
        if budget == 0 {
            return ledger_progress(progress, next_block, ledger_tip, None);
        }

        let request = GetBlocksRequest {
            start: Nat::from(next_block),
            length: Nat::from(ledger_page_length(next_block, ledger_tip, PAGE)),
        };
        budget -= 1;
        let response = match ledger_call(ledger, "get_transactions")
            .with_arg(&request)
            .await
            .ok()
            .and_then(|response| response.candid::<GetTransactionsResponse>().ok())
        {
            Some(response) => response,
            None => return ledger_progress(progress, next_block, ledger_tip, None),
        };
        let Some(log_length) = nat_u128(&response.log_length) else {
            return ledger_progress(progress, next_block, ledger_tip, None);
        };
        let fixed_tip = *ledger_tip.get_or_insert(log_length);
        if log_length < fixed_tip {
            return ledger_progress(progress, next_block, ledger_tip, None);
        }
        let requested_end = fixed_tip.min(next_block.saturating_add(PAGE));
        let Some(first_index) = nat_u128(&response.first_index) else {
            return ledger_progress(progress, next_block, ledger_tip, None);
        };
        for (offset, transaction) in response.transactions.iter().enumerate() {
            if matches_identity(transaction, &progress.transfer) {
                return ReconciliationOutcome::Succeeded {
                    block_index: first_index + offset as u128,
                };
            }
        }

        let mut coverage = Vec::with_capacity(response.archived_transactions.len() + 1);
        if !response.transactions.is_empty() {
            let Some(end) = first_index.checked_add(response.transactions.len() as u128) else {
                return ledger_progress(progress, next_block, ledger_tip, None);
            };
            coverage.push((first_index, end));
        }
        let mut archives = Vec::with_capacity(response.archived_transactions.len());
        for archived in response.archived_transactions {
            let (Some(start), Some(length)) =
                (nat_u128(&archived.start), nat_u128(&archived.length))
            else {
                return ledger_progress(progress, next_block, ledger_tip, None);
            };
            let Some(end) = start.checked_add(length) else {
                return ledger_progress(progress, next_block, ledger_tip, None);
            };
            if length == 0 || archived.callback.method.len() > 128 {
                return ledger_progress(progress, next_block, ledger_tip, None);
            }
            coverage.push((start, end));
            archives.push(ReconciliationArchiveRange {
                canister_id: archived.callback.canister_id.as_slice().to_vec(),
                method: archived.callback.method,
                start,
                length,
            });
        }
        coverage.sort_unstable();
        if !exact_coverage(next_block, requested_end, &coverage) {
            return ledger_progress(progress, next_block, ledger_tip, None);
        }
        archives.sort_unstable_by_key(|archive| archive.start);
        pending_page = Some(ReconciliationLedgerPage {
            end: requested_end,
            archives,
            next_archive: 0,
        });
    }
}

fn ledger_page_length(next_block: u128, ledger_tip: Option<u128>, page: u128) -> u128 {
    ledger_tip
        .map(|exclusive_tip| exclusive_tip.saturating_sub(next_block).min(page))
        .unwrap_or(page)
}

fn ledger_progress(
    progress: &mut ReconciliationScanProgress,
    next_block: u128,
    ledger_tip: Option<u128>,
    pending_page: Option<ReconciliationLedgerPage>,
) -> ReconciliationOutcome {
    progress.phase = ReconciliationScanPhase::Ledger {
        next_block,
        ledger_tip,
        pending_page: pending_page.map(Box::new),
    };
    ReconciliationOutcome::Progress(Box::new(progress.clone()))
}

fn exact_coverage(start: u128, end: u128, ranges: &[(u128, u128)]) -> bool {
    if start == end {
        return ranges.is_empty();
    }
    let mut cursor = start;
    for &(range_start, range_end) in ranges {
        if range_start != cursor || range_end <= range_start || range_end > end {
            return false;
        }
        cursor = range_end;
    }
    cursor == end
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IndexPageBoundary {
    Continue(u128),
    Exhausted,
    Stalled,
}

fn index_page_boundary(
    previous_start: Option<u128>,
    last: u128,
    oldest: Option<u128>,
) -> IndexPageBoundary {
    if oldest.is_some_and(|oldest| last <= oldest) {
        IndexPageBoundary::Exhausted
    } else if previous_start == Some(last) {
        IndexPageBoundary::Stalled
    } else {
        // The index start cursor is exclusive, so the last returned id is the
        // boundary for the next descending page; subtracting one would skip it.
        IndexPageBoundary::Continue(last)
    }
}

async fn reconcile_index(
    index: Principal,
    expected_ledger: Principal,
    progress: &mut ReconciliationScanProgress,
    ledger_watermark: u128,
    mut index_watermark: Option<u128>,
    mut next_start: Option<u128>,
    mut budget: u8,
) -> ReconciliationOutcome {
    if index_watermark.is_none() {
        if budget < 2 {
            return index_progress(progress, ledger_watermark, None, next_start);
        }
        budget -= 1;
        let ledger_matches = ledger_call(index, "ledger_id")
            .with_arg(())
            .await
            .ok()
            .and_then(|response| response.candid::<Principal>().ok())
            .is_some_and(|id| id == expected_ledger);
        if !ledger_matches {
            return index_progress(progress, ledger_watermark, None, next_start);
        }

        budget -= 1;
        let status = match ledger_call(index, "status")
            .with_arg(())
            .await
            .ok()
            .and_then(|response| response.candid::<IndexStatus>().ok())
        {
            Some(status) => status,
            None => return index_progress(progress, ledger_watermark, None, next_start),
        };
        let Some(observed_watermark) = nat_u128(&status.num_blocks_synced) else {
            return index_progress(progress, ledger_watermark, None, next_start);
        };
        if observed_watermark < ledger_watermark {
            return index_progress(progress, ledger_watermark, None, next_start);
        }
        index_watermark = Some(observed_watermark);
    }

    let account = Account {
        owner: Principal::from_slice(progress.transfer.from.owner()),
        subaccount: Some(progress.transfer.from.subaccount()),
    };
    while budget > 0 {
        let args = GetAccountTransactionsArgs {
            account,
            start: next_start.map(Nat::from),
            max_results: Nat::from(100u16),
        };
        budget -= 1;
        let result = match ledger_call(index, "get_account_transactions")
            .with_arg(&args)
            .await
            .ok()
            .and_then(|response| {
                response
                    .candid::<Result<IndexTransactions, IndexError>>()
                    .ok()
            }) {
            Some(Ok(result)) => result,
            _ => return index_progress(progress, ledger_watermark, index_watermark, next_start),
        };
        for transaction in &result.transactions {
            if matches_identity(&transaction.transaction, &progress.transfer) {
                return match nat_u128(&transaction.id) {
                    Some(block_index) => ReconciliationOutcome::Succeeded { block_index },
                    None => index_progress(progress, ledger_watermark, index_watermark, next_start),
                };
            }
        }
        let Some(last) = result.transactions.last().and_then(|tx| nat_u128(&tx.id)) else {
            return ReconciliationOutcome::Absent {
                ledger_watermark,
                index_watermark: index_watermark.expect("verified index watermark"),
            };
        };
        match index_page_boundary(
            next_start,
            last,
            result.oldest_tx_id.as_ref().and_then(nat_u128),
        ) {
            IndexPageBoundary::Exhausted => {
                return ReconciliationOutcome::Absent {
                    ledger_watermark,
                    index_watermark: index_watermark.expect("verified index watermark"),
                };
            }
            IndexPageBoundary::Stalled => {
                return index_progress(progress, ledger_watermark, index_watermark, next_start);
            }
            IndexPageBoundary::Continue(start) => next_start = Some(start),
        }
    }
    index_progress(progress, ledger_watermark, index_watermark, next_start)
}

fn index_progress(
    progress: &mut ReconciliationScanProgress,
    ledger_watermark: u128,
    index_watermark: Option<u128>,
    next_start: Option<u128>,
) -> ReconciliationOutcome {
    progress.phase = ReconciliationScanPhase::Index {
        ledger_watermark,
        index_watermark,
        next_start,
    };
    ReconciliationOutcome::Progress(Box::new(progress.clone()))
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
    fn every_ledger_call_uses_the_fixed_fifteen_second_bound() {
        assert_eq!(LEDGER_CALL_TIMEOUT_SECONDS, 15);
    }

    #[test]
    fn kinic_ledger_fee_matches_the_immutable_deployment_parameter() {
        assert_eq!(KINIC_LEDGER_FEE, Amount::new(10_000));
    }

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

    #[test]
    fn ledger_page_coverage_rejects_gaps_overlaps_and_out_of_range_segments() {
        assert!(exact_coverage(0, 10, &[(0, 4), (4, 10)]));
        assert!(exact_coverage(5, 5, &[]));
        assert!(!exact_coverage(0, 10, &[(0, 4), (5, 10)]));
        assert!(!exact_coverage(0, 10, &[(0, 6), (5, 10)]));
        assert!(!exact_coverage(0, 10, &[(0, 11)]));
    }

    #[test]
    fn fixed_ledger_tip_truncates_the_final_page_when_the_ledger_grows() {
        assert_eq!(ledger_page_length(0, None, 1_000), 1_000);
        assert_eq!(ledger_page_length(1_000, Some(2_500), 1_000), 1_000);
        assert_eq!(ledger_page_length(2_000, Some(2_500), 1_000), 500);
        assert_eq!(ledger_page_length(2_500, Some(2_500), 1_000), 0);

        // The official ledger and archive APIs return half-open ranges. Keeping the
        // final request inside the fixed tip makes their combined coverage exact,
        // even if the live ledger has advanced beyond that tip.
        assert!(exact_coverage(
            2_000,
            2_500,
            &[(2_000, 2_200), (2_200, 2_500)]
        ));
        assert!(!exact_coverage(
            2_000,
            2_500,
            &[(2_000, 2_200), (2_200, 3_000)]
        ));
    }

    #[test]
    fn index_pages_use_the_last_id_as_an_exclusive_descending_boundary() {
        assert_eq!(
            index_page_boundary(None, 900, Some(0)),
            IndexPageBoundary::Continue(900)
        );
        assert_eq!(
            index_page_boundary(Some(900), 800, Some(0)),
            IndexPageBoundary::Continue(800)
        );
        assert_eq!(
            index_page_boundary(Some(1), 0, Some(0)),
            IndexPageBoundary::Exhausted
        );
        assert_eq!(
            index_page_boundary(Some(800), 800, Some(0)),
            IndexPageBoundary::Stalled
        );
    }
}
