use crate::{api, STORE};
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

pub fn consent_message(
    caller: Principal,
    canister: Principal,
    request: Icrc21ConsentMessageRequest,
    current_ledger_fee: Option<u128>,
) -> Icrc21ConsentMessageResponse {
    if request.method == "continue_deposit" || request.method == "continue_withdrawal" {
        return settlement_consent(caller, canister, request);
    }
    if request.method == "continue_fee_payout" {
        return fee_payout_consent(caller, canister, request);
    }
    if request.method == "notify_withdrawal" {
        return withdrawal_consent(caller, canister, request);
    }
    if request.method != "request_deposit" {
        return unsupported(format!("unsupported canister call: {}", request.method));
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
    let Some(refund_amount) = validated.gross_amount.checked_sub(ledger_fee) else {
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
            "# Bridge KINIC to Base\n\nSource wallet: `{caller}`\n\nOwner sequence: `{owner_sequence}`\n\nSource subaccount: `{subaccount}`\n\nGross bridge amount: `{gross}` KINIC\n\nLedger transfer fee: `{ledger_fee}` KINIC\n\nTotal wallet debit: `{total_debit}` KINIC\n\nMaximum service fee: `{fee}` KINIC\n\nMinimum Base amount: `{minimum}` KINIC\n\nBase chain ID: `{base_chain_id}`\n\nBase recipient: `0x{recipient}`\n\nBridge canister: `{canister}`\n\nThe Bridge canister will pull the displayed total using an existing ICRC-2 allowance. After the pull, a Base Mint Authorization remains irrevocable for {authorization_minutes} minutes. You need a Base wallet and Base ETH to submit it. If it is unused, the IC refund starts only after the Base Finalized timestamp has passed the deadline and the deposit is proven unprocessed. The refund is `{refund_amount}` KINIC (gross minus the fixed Ledger fee); `{ledger_fee}` KINIC is paid from escrow as the refund fee.\n\n**bSNS does not provide SNS voting rights or SNS voting rewards.**",
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
    if caller == Principal::anonymous() {
        return unavailable("anonymous caller is not allowed");
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
}
