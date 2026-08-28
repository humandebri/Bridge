use crate::{admin, api, STORE};
use candid::{CandidType, Decode, Deserialize, Nat, Principal};

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct Icrc21ConsentMessageRequest {
    pub arg: Vec<u8>,
    pub method: String,
    pub user_preferences: Icrc21ConsentMessageSpec,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct Icrc21ConsentMessageSpec {
    pub metadata: Icrc21ConsentMessageMetadata,
    pub device_spec: Option<Icrc21DeviceSpec>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct Icrc21ConsentMessageMetadata {
    pub utc_offset_minutes: Option<i16>,
    pub language: String,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub enum Icrc21DeviceSpec {
    GenericDisplay,
    FieldsDisplay,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub enum Icrc21ConsentMessage {
    GenericDisplayMessage(String),
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct Icrc21ConsentInfo {
    pub metadata: Icrc21ConsentMessageMetadata,
    pub consent_message: Icrc21ConsentMessage,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct Icrc21ErrorInfo {
    pub description: String,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct Icrc21GenericError {
    pub description: String,
    pub error_code: Nat,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub enum Icrc21Error {
    GenericError(Icrc21GenericError),
    InsufficientPayment(Icrc21ErrorInfo),
    UnsupportedCanisterCall(Icrc21ErrorInfo),
    ConsentMessageUnavailable(Icrc21ErrorInfo),
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub enum Icrc21ConsentMessageResponse {
    Ok(Icrc21ConsentInfo),
    Err(Icrc21Error),
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub struct Icrc10SupportedStandard {
    pub name: String,
    pub url: String,
}

pub fn supported_standards() -> Vec<Icrc10SupportedStandard> {
    vec![Icrc10SupportedStandard {
        name: "ICRC-21".into(),
        url: "https://github.com/dfinity/ICRC/blob/main/ICRCs/ICRC-21/ICRC-21.md".into(),
    }]
}

pub fn resource_limited() -> Icrc21ConsentMessageResponse {
    Icrc21ConsentMessageResponse::Err(Icrc21Error::GenericError(Icrc21GenericError {
        description: "Consent message capacity is temporarily unavailable.".into(),
        error_code: Nat::from(429u16),
    }))
}

pub fn consent_message(
    caller: Principal,
    canister: Principal,
    request: Icrc21ConsentMessageRequest,
    current_ledger_fee: Option<u128>,
) -> Icrc21ConsentMessageResponse {
    const MAX_METHOD_BYTES: usize = 64;
    const MAX_ARGUMENT_BYTES: usize = 4_096;
    const MAX_LANGUAGE_BYTES: usize = 16;
    if request.method.len() > MAX_METHOD_BYTES
        || request.arg.len() > MAX_ARGUMENT_BYTES
        || request.user_preferences.metadata.language.len() > MAX_LANGUAGE_BYTES
    {
        return unavailable("consent request exceeds supported size limits");
    }
    if request.method == "continue_deposit" || request.method == "continue_withdrawal" {
        return settlement_consent(caller, canister, request);
    }
    if request.method == "continue_fee_payout" {
        return fee_payout_consent(caller, canister, request);
    }
    if request.method == "notify_withdrawal" {
        return withdrawal_consent(caller, canister, request);
    }
    if request.method == "request_deposit_refund" {
        return deposit_refund_consent(caller, canister, request, current_ledger_fee);
    }
    if request.method != "request_deposit" {
        return unsupported("unsupported canister call");
    }
    let args = match Decode!(&request.arg, api::DepositArgs) {
        Ok(args) => args,
        Err(error) => {
            return unavailable(format!("request_deposit argument decode failed: {error}"));
        }
    };
    let validated = match api::validate_deposit_args(caller, &args) {
        Ok(validated) => validated,
        Err(error) => return unavailable(format!("invalid request_deposit call: {error:?}")),
    };
    let base_chain_id = match STORE.with(|store| store.borrow().config()) {
        Ok(Some(config)) => config.base_chain_id,
        Ok(None) => return unavailable("bridge configuration is unavailable"),
        Err(error) => return unavailable(format!("bridge configuration read failed: {error}")),
    };
    let language = if request.user_preferences.metadata.language.trim().is_empty() {
        "en".to_string()
    } else {
        request.user_preferences.metadata.language
    };
    let minimum = validated
        .gross_amount
        .saturating_sub(validated.max_service_fee);
    let Some(ledger_fee) = current_ledger_fee else {
        return unavailable("current ledger fee is unavailable");
    };
    let Some(total_debit) = validated.gross_amount.checked_add(ledger_fee) else {
        return unavailable("deposit total debit exceeds u128");
    };
    let Some(refund_amount) = validated
        .gross_amount
        .checked_sub(validated.max_service_fee)
        .and_then(|amount| amount.checked_sub(ledger_fee))
    else {
        return unavailable("deposit amount does not cover the fixed refund fee");
    };
    let subaccount = if validated.from_subaccount == [0; 32] {
        "default (32 zero bytes)".to_string()
    } else {
        format!("0x{}", hex(&validated.from_subaccount))
    };
    Icrc21ConsentMessageResponse::Ok(Icrc21ConsentInfo {
        metadata: Icrc21ConsentMessageMetadata {
            language,
            utc_offset_minutes: request.user_preferences.metadata.utc_offset_minutes,
        },
        consent_message: Icrc21ConsentMessage::GenericDisplayMessage(format!(
            "# Bridge KINIC to Base\n\nSource wallet: `{caller}`\n\nOwner sequence: `{owner_sequence}`\n\nSource subaccount: `{subaccount}`\n\nGross bridge amount: `{gross}` KINIC\n\nLedger transfer fee: `{ledger_fee}` KINIC\n\nTotal wallet debit: `{total_debit}` KINIC\n\nMaximum service fee: `{fee}` KINIC\n\nMinimum Base amount: `{minimum}` KINIC\n\nBase chain ID: `{base_chain_id}`\n\nBase recipient: `0x{recipient}`\n\nBridge canister: `{canister}`\n\nThe Bridge canister will pull the displayed total using an existing ICRC-2 allowance. After the pull, the Canister issues a Base Mint Authorization that is valid for {authorization_minutes} minutes from its IC consensus issue time. At least five minutes must remain before the Canister installs its signature or the UI submits it. You need a Base wallet and Base ETH to submit the Base transaction. Installing the signature permanently earns the displayed service fee. The initial pull Ledger fee is not refundable. If the authorization expires unused, no automatic transfer occurs: any non-anonymous Principal may advance the refund only after the Base Finalized timestamp has passed the deadline and the exact deposit remains unprocessed. The destination, amount, and Ledger transfer identity remain fixed by this deposit. The minimum refund after authorization is `{refund_amount}` KINIC after the maximum service fee and a second fixed Ledger fee are deducted.\n\n**bSNS does not provide SNS voting rights or SNS voting rewards.**",
            owner_sequence = validated.owner_sequence,
            gross = format_e8s(validated.gross_amount),
            ledger_fee = format_e8s(ledger_fee),
            total_debit = format_e8s(total_debit),
            fee = format_e8s(validated.max_service_fee),
            minimum = format_e8s(minimum),
            refund_amount = format_e8s(refund_amount),
            recipient = hex(&validated.base_recipient),
            authorization_minutes = bridge_core::MINT_AUTHORIZATION_TTL_SECONDS / 60,
        )),
    })
}

fn settlement_consent(
    caller: Principal,
    canister: Principal,
    request: Icrc21ConsentMessageRequest,
) -> Icrc21ConsentMessageResponse {
    if caller == Principal::anonymous() {
        return unavailable("anonymous caller is not allowed");
    }
    let id = match Decode!(&request.arg, Vec<u8>) {
        Ok(id) if id.len() == 32 => id,
        Ok(_) => return unavailable("settlement ID must be 32 bytes"),
        Err(error) => return unavailable(format!("settlement ID decode failed: {error}")),
    };
    Icrc21ConsentMessageResponse::Ok(Icrc21ConsentInfo {
        metadata: request.user_preferences.metadata,
        consent_message: Icrc21ConsentMessage::GenericDisplayMessage(format!(
            "# Retry bridge settlement\n\nIC wallet: `{caller}`\n\nAction: `{method}`\n\nSettlement ID: `0x{id}`\n\nBridge canister: `{canister}`\n\nThis manual recovery call retries immediately available work after settlement has stopped. Submitted Base transactions require the dedicated wallet-confirmed confirmation call.",
            method = request.method,
            id = hex(&id),
        )),
    })
}

fn fee_payout_consent(
    caller: Principal,
    canister: Principal,
    request: Icrc21ConsentMessageRequest,
) -> Icrc21ConsentMessageResponse {
    if !matches!(admin::can_manage_fee_payout(caller), Ok(true)) {
        return unavailable("fee payout consent is not authorized");
    }
    let payout_id = match Decode!(&request.arg, u64) {
        Ok(id) => id,
        Err(error) => return unavailable(format!("fee payout ID decode failed: {error}")),
    };
    let payout = match STORE.with(|store| store.borrow().fee_payout(payout_id)) {
        Ok(Some(payout)) => payout,
        Ok(None) => return unavailable("fee payout does not exist"),
        Err(error) => return unavailable(format!("fee payout read failed: {error}")),
    };
    let subaccount = if payout.recipient.subaccount.is_empty() {
        "default".to_owned()
    } else {
        format!("0x{}", hex(&payout.recipient.subaccount))
    };
    Icrc21ConsentMessageResponse::Ok(Icrc21ConsentInfo {
        metadata: request.user_preferences.metadata,
        consent_message: Icrc21ConsentMessage::GenericDisplayMessage(format!(
            "# Continue fee payout\n\nAdministrator: `{caller}`\n\nPayout ID: `{payout_id}`\n\nAmount: `{amount}` KINIC\n\nRecipient: `{recipient}`\n\nRecipient subaccount: `{subaccount}`\n\nBridge canister: `{canister}`\n\nThis call performs one explicit payout or reconciliation step and does not schedule an automatic retry.",
            amount = format_e8s(payout.amount),
            recipient = payout.recipient.owner,
        )),
    })
}

fn withdrawal_consent(
    caller: Principal,
    canister: Principal,
    request: Icrc21ConsentMessageRequest,
) -> Icrc21ConsentMessageResponse {
    if caller == Principal::anonymous() {
        return unavailable("anonymous caller is not allowed");
    }
    let args = match Decode!(&request.arg, api::NotifyWithdrawalArgs) {
        Ok(args) => args,
        Err(error) => {
            return unavailable(format!("notify_withdrawal argument decode failed: {error}"));
        }
    };
    let transaction_hash: [u8; 32] = match args.transaction_hash.as_slice().try_into() {
        Ok(hash) => hash,
        Err(_) => return unavailable("transaction_hash must be 32 bytes"),
    };
    let config = match STORE.with(|store| store.borrow().config()) {
        Ok(Some(config)) => config,
        Ok(None) => return unavailable("bridge configuration is unavailable"),
        Err(error) => return unavailable(format!("bridge configuration read failed: {error}")),
    };
    let language = if request.user_preferences.metadata.language.trim().is_empty() {
        "en".to_string()
    } else {
        request.user_preferences.metadata.language
    };
    Icrc21ConsentMessageResponse::Ok(Icrc21ConsentInfo {
        metadata: Icrc21ConsentMessageMetadata {
            language,
            utc_offset_minutes: request.user_preferences.metadata.utc_offset_minutes,
        },
        consent_message: Icrc21ConsentMessage::GenericDisplayMessage(format!(
            "# Notify a finalized Base withdrawal\n\nIC wallet: `{caller}`\n\nBase transaction: `0x{transaction_hash}`\n\nBase chain ID: `{base_chain_id}`\n\nBridge contract: `0x{bridge_contract}`\n\nBridge canister: `{canister}`\n\nThe Bridge canister independently verifies the finalized canonical receipt, Bridge contract, WithdrawalCommitted event, fixed amount out, service fee, and event owner. The Base burn is irreversible and interrupted delivery is retried only to the committed IC account.",
            transaction_hash = hex(&transaction_hash),
            base_chain_id = config.base_chain_id,
            bridge_contract = hex(&config.bridge_contract),
        )),
    })
}

fn deposit_refund_consent(
    caller: Principal,
    canister: Principal,
    request: Icrc21ConsentMessageRequest,
    current_ledger_fee: Option<u128>,
) -> Icrc21ConsentMessageResponse {
    if caller == Principal::anonymous() {
        return unavailable("anonymous caller is not allowed");
    }
    let deposit_id = match Decode!(&request.arg, Vec<u8>) {
        Ok(id) if id.len() == 32 => id,
        Ok(_) => return unavailable("deposit_id must be 32 bytes"),
        Err(error) => {
            return unavailable(format!(
                "request_deposit_refund argument decode failed: {error}"
            ));
        }
    };
    let deposit_id: [u8; 32] = match deposit_id.as_slice().try_into() {
        Ok(id) => id,
        Err(_) => return unavailable("deposit_id must be 32 bytes"),
    };
    let record = match STORE.with(|store| store.borrow().deposit(deposit_id)) {
        Ok(Some(record)) if record.transfer.from.owner() == caller.as_slice() => record,
        Ok(Some(_)) => return unavailable("caller is not the deposit owner"),
        Ok(None) => return unavailable("deposit does not exist"),
        Err(error) => return unavailable(format!("deposit read failed: {error}")),
    };
    let message = match deposit_refund_consent_message(
        caller,
        canister,
        deposit_id,
        &record,
        current_ledger_fee,
    ) {
        Ok(message) => message,
        Err(error) => return unavailable(error),
    };
    Icrc21ConsentMessageResponse::Ok(Icrc21ConsentInfo {
        metadata: request.user_preferences.metadata,
        consent_message: Icrc21ConsentMessage::GenericDisplayMessage(message),
    })
}

fn deposit_refund_consent_message(
    caller: Principal,
    canister: Principal,
    deposit_id: [u8; 32],
    record: &bridge_core::DepositRecord,
    current_ledger_fee: Option<u128>,
) -> Result<String, String> {
    let service_fee = if record
        .mint_authorization
        .as_ref()
        .is_some_and(|authorization| authorization.signature.is_some())
    {
        record.quote.map_or(0, |quote| quote.service_fee.get())
    } else {
        0
    };
    let computed_refund = || -> Result<(u128, u128), String> {
        let ledger_fee =
            current_ledger_fee.ok_or_else(|| "current ledger fee is unavailable".to_string())?;
        let refund_amount =
            bridge_core::deposit_refund_amount(record.gross_amount.get(), service_fee, ledger_fee)
                .ok_or_else(|| {
                    "deposit amount does not cover the non-refundable fees".to_string()
                })?;
        Ok((refund_amount, ledger_fee))
    };

    let (title, amount_line, explanation) = match &record.state {
        bridge_core::DepositState::AuthorizationPending { .. }
        | bridge_core::DepositState::AuthorizationAvailable { .. } => {
            let (refund_amount, ledger_fee) = computed_refund()?;
            (
                "# Check IC refund eligibility",
                format!(
                    "Potential refund if eligible: `{}` KINIC\n\nPotential non-refundable refund Ledger fee: `{}` KINIC",
                    format_e8s(refund_amount),
                    format_e8s(ledger_fee),
                ),
                "This call first checks one canonical Base Finalized observation. It starts a refund only after the authorization deadline has passed and the deposit is still unprocessed. A processed deposit is marked minted and no Ledger transfer is made.",
            )
        }
        bridge_core::DepositState::RefundAvailable { .. } => {
            let (refund_amount, ledger_fee) = computed_refund()?;
            (
                "# Start IC refund",
                format!(
                    "Refund to send: `{}` KINIC\n\nNon-refundable refund Ledger fee: `{}` KINIC",
                    format_e8s(refund_amount),
                    format_e8s(ledger_fee),
                ),
                "This call starts the displayed refund transfer. It performs one explicit settlement step and does not promise completion before the Ledger result is known.",
            )
        }
        bridge_core::DepositState::RefundPending { attempt, .. }
        | bridge_core::DepositState::RefundReconciliationHold { attempt, .. } => (
            "# Continue IC refund",
            format!(
                "Committed refund amount: `{}` KINIC\n\nCommitted non-refundable refund Ledger fee: `{}` KINIC",
                format_e8s(attempt.identity.amount.get()),
                format_e8s(attempt.identity.fee.get()),
            ),
            "This call continues reconciliation for the same committed refund. It may query the Ledger or submit the displayed transfer only when prior absence is established.",
        ),
        bridge_core::DepositState::Minted { .. }
        | bridge_core::DepositState::Refunded { .. } => {
            return Err("refund consent is unavailable for a terminal deposit".into());
        }
        bridge_core::DepositState::FundingPending
        | bridge_core::DepositState::EscrowedUnquoted { .. }
        | bridge_core::DepositState::FundingReconciliationHold { .. }
        | bridge_core::DepositState::Cancelled { .. } => {
            return Err("refund consent is unavailable for the current deposit state".into());
        }
    };

    Ok(format!(
        "{title}\n\nIC wallet: `{caller}`\n\nDeposit ID: `0x{deposit_id}`\n\nGross deposit: `{gross}` KINIC\n\nNon-refundable service fee: `{service_fee}` KINIC\n\n{amount_line}\n\nBridge canister: `{canister}`\n\n{explanation}",
        deposit_id = hex(&deposit_id),
        gross = format_e8s(record.gross_amount.get()),
        service_fee = format_e8s(service_fee),
    ))
}

fn format_e8s(value: u128) -> String {
    let whole = value / 100_000_000;
    let fraction = value % 100_000_000;
    if fraction == 0 {
        return whole.to_string();
    }
    format!("{whole}.{fraction:08}")
        .trim_end_matches('0')
        .to_string()
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn unsupported(description: impl Into<String>) -> Icrc21ConsentMessageResponse {
    Icrc21ConsentMessageResponse::Err(Icrc21Error::UnsupportedCanisterCall(Icrc21ErrorInfo {
        description: description.into(),
    }))
}

fn unavailable(description: impl Into<String>) -> Icrc21ConsentMessageResponse {
    Icrc21ConsentMessageResponse::Err(Icrc21Error::ConsentMessageUnavailable(Icrc21ErrorInfo {
        description: description.into(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::{
        Account, Amount, DepositId, DepositRecord, DepositRequest, DepositState, LedgerOperation,
        LedgerTransferIdentity, TransferAttempt,
    };
    use candid::Encode;

    fn request(args: &api::DepositArgs) -> Icrc21ConsentMessageRequest {
        Icrc21ConsentMessageRequest {
            arg: Encode!(args).expect("encode deposit args"),
            method: "request_deposit".into(),
            user_preferences: Icrc21ConsentMessageSpec {
                metadata: Icrc21ConsentMessageMetadata {
                    utc_offset_minutes: None,
                    language: "en".into(),
                },
                device_spec: Some(Icrc21DeviceSpec::GenericDisplay),
            },
        }
    }

    fn settlement_request(method: &str, id: Vec<u8>) -> Icrc21ConsentMessageRequest {
        Icrc21ConsentMessageRequest {
            arg: Encode!(&id).expect("encode settlement ID"),
            method: method.into(),
            user_preferences: request(&api::DepositArgs {
                owner_sequence: 0,
                base_recipient: vec![2; 20],
                from_subaccount: None,
                gross_amount: Nat::from(200_000_000u64),
                max_service_fee: Nat::from(1_000_000u64),
            })
            .user_preferences,
        }
    }

    fn account(tag: u8) -> Account {
        Account::new(vec![tag], [tag; 32]).expect("valid test account")
    }

    fn deposit(state: DepositState) -> DepositRecord {
        let transfer = LedgerTransferIdentity {
            operation: LedgerOperation::PullDeposit,
            created_at_time_ns: 1,
            memo: [1; 32],
            amount: Amount::new(200_000_000),
            fee: Amount::new(10_000),
            from: account(1),
            to: account(2),
            spender: Some(account(3)),
        };
        let mut record = DepositRecord::accept(DepositRequest {
            id: DepositId::new([7; 32]),
            payload_hash: [8; 32],
            gross_amount: Amount::new(200_000_000),
            user_max_service_fee: Amount::new(1_000_000),
            transfer,
        })
        .expect("valid test deposit");
        record.state = state;
        record
    }

    fn refund_attempt() -> TransferAttempt {
        TransferAttempt {
            attempt_no: 0,
            identity: LedgerTransferIdentity {
                operation: LedgerOperation::RefundDeposit,
                created_at_time_ns: 2,
                memo: [2; 32],
                amount: Amount::new(199_990_000),
                fee: Amount::new(10_000),
                from: account(2),
                to: account(1),
                spender: None,
            },
        }
    }

    #[test]
    fn rejects_malformed_and_unsupported_calls() {
        let caller = Principal::management_canister();
        let malformed = Icrc21ConsentMessageRequest {
            arg: Vec::new(),
            method: "request_deposit".into(),
            user_preferences: request(&api::DepositArgs {
                owner_sequence: 0,
                base_recipient: vec![2; 20],
                from_subaccount: None,
                gross_amount: Nat::from(200_000_000u64),
                max_service_fee: Nat::from(1_000_000u64),
            })
            .user_preferences,
        };
        assert!(matches!(
            consent_message(caller, caller, malformed, Some(10_000)),
            Icrc21ConsentMessageResponse::Err(Icrc21Error::ConsentMessageUnavailable(_))
        ));

        let reflected = "x".repeat(32);
        let mut unsupported_request = request(&api::DepositArgs {
            owner_sequence: 0,
            base_recipient: vec![2; 20],
            from_subaccount: None,
            gross_amount: Nat::from(200_000_000u64),
            max_service_fee: Nat::from(1_000_000u64),
        });
        unsupported_request.method = reflected.clone();
        let response = consent_message(caller, caller, unsupported_request, Some(10_000));
        assert!(!format!("{response:?}").contains(&reflected));

        let mut oversized = request(&api::DepositArgs {
            owner_sequence: 0,
            base_recipient: vec![2; 20],
            from_subaccount: None,
            gross_amount: Nat::from(200_000_000u64),
            max_service_fee: Nat::from(1_000_000u64),
        });
        oversized.arg = vec![0; 4_097];
        assert!(matches!(
            consent_message(caller, caller, oversized, Some(10_000)),
            Icrc21ConsentMessageResponse::Err(Icrc21Error::ConsentMessageUnavailable(_))
        ));

        let mut unsupported_request = request(&api::DepositArgs {
            owner_sequence: 0,
            base_recipient: vec![2; 20],
            from_subaccount: None,
            gross_amount: Nat::from(200_000_000u64),
            max_service_fee: Nat::from(1_000_000u64),
        });
        unsupported_request.method = "get_bridge_status".into();
        assert!(matches!(
            consent_message(caller, caller, unsupported_request, Some(10_000)),
            Icrc21ConsentMessageResponse::Err(Icrc21Error::UnsupportedCanisterCall(_))
        ));

        let withdrawal_request = Icrc21ConsentMessageRequest {
            arg: Encode!(&api::NotifyWithdrawalArgs {
                transaction_hash: vec![1; 31],
            })
            .expect("encode withdrawal notification"),
            method: "notify_withdrawal".into(),
            user_preferences: request(&api::DepositArgs {
                owner_sequence: 0,
                base_recipient: vec![2; 20],
                from_subaccount: None,
                gross_amount: Nat::from(200_000_000u64),
                max_service_fee: Nat::from(1_000_000u64),
            })
            .user_preferences,
        };
        assert!(matches!(
            consent_message(caller, caller, withdrawal_request, Some(10_000)),
            Icrc21ConsentMessageResponse::Err(Icrc21Error::ConsentMessageUnavailable(_))
        ));
    }

    #[test]
    fn continue_deposit_consent_requires_an_authenticated_caller_and_exact_id() {
        let caller = Principal::management_canister();
        let canister = Principal::from_slice(&[9]);
        let response = consent_message(
            caller,
            canister,
            settlement_request("continue_deposit", vec![7; 32]),
            None,
        );
        let Icrc21ConsentMessageResponse::Ok(info) = response else {
            panic!("valid continue_deposit consent must be available");
        };
        let Icrc21ConsentMessage::GenericDisplayMessage(message) = info.consent_message;
        assert!(message.contains("Action: `continue_deposit`"));
        assert!(message.contains(&format!("Settlement ID: `0x{}`", "07".repeat(32))));

        for invalid_id in [vec![7; 31], vec![7; 33]] {
            assert!(matches!(
                consent_message(
                    caller,
                    canister,
                    settlement_request("continue_deposit", invalid_id),
                    None,
                ),
                Icrc21ConsentMessageResponse::Err(Icrc21Error::ConsentMessageUnavailable(_))
            ));
        }
        assert!(matches!(
            consent_message(
                Principal::anonymous(),
                canister,
                settlement_request("continue_deposit", vec![7; 32]),
                None,
            ),
            Icrc21ConsentMessageResponse::Err(Icrc21Error::ConsentMessageUnavailable(_))
        ));
    }

    #[test]
    fn describes_refund_actions_without_promising_receipt() {
        let caller = Principal::management_canister();
        let canister = caller;
        let pending = deposit(DepositState::AuthorizationAvailable {
            funding_ledger_block_index: 1,
        });
        let message =
            deposit_refund_consent_message(caller, canister, [7; 32], &pending, Some(10_000))
                .expect("authorization consent");
        assert!(message.contains("# Check IC refund eligibility"));
        assert!(message.contains("Potential refund if eligible"));
        assert!(!message.contains("Refund received"));

        let refunding = deposit(DepositState::RefundPending {
            reason: bridge_core::DepositRefundReason::BasePaused,
            funding_ledger_block_index: 2,
            attempt: refund_attempt(),
        });
        let message = deposit_refund_consent_message(caller, canister, [7; 32], &refunding, None)
            .expect("pending refund consent uses the committed attempt");
        assert!(message.contains("# Continue IC refund"));
        assert!(message.contains("Committed refund amount"));
        assert!(message.contains("1.9999"));
    }

    #[test]
    fn terminal_refund_consent_is_rejected() {
        let caller = Principal::management_canister();
        let minted = deposit(DepositState::Minted {
            funding_ledger_block_index: 3,
        });
        assert!(deposit_refund_consent_message(caller, caller, [7; 32], &minted, None).is_err());

        let refunded = deposit(DepositState::Refunded {
            reason: bridge_core::DepositRefundReason::BasePaused,
            funding_ledger_block_index: 3,
            attempt: refund_attempt(),
            refund_ledger_block_index: 4,
            source_hold: None,
        });
        assert!(deposit_refund_consent_message(caller, caller, [7; 32], &refunded, None).is_err());

        let unsupported = deposit(DepositState::FundingPending);
        assert!(deposit_refund_consent_message(
            caller,
            caller,
            [7; 32],
            &unsupported,
            Some(10_000)
        )
        .is_err());
    }
}
