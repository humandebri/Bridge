use crate::{config::BridgeInitArgs, STORE};
use bridge_core::GovernanceTransactionEnvelope;
use candid::{utils::ArgumentEncoder, CandidType, Principal};
use ic_cdk::call::Call;
use ic_cdk_management_canister::{
    cost_sign_with_ecdsa, EcdsaCurve, EcdsaKeyId, EcdsaPublicKeyArgs, EcdsaPublicKeyResult,
    SignCallError, SignWithEcdsaArgs,
};
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use serde::de::DeserializeOwned;
use tiny_keccak::{Hasher, Keccak};

const PUBLIC_KEY_CALL_TIMEOUT_SECONDS: u32 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignerRole {
    Mint,
    Governance,
    RuntimeAdministrator,
    Canceller,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlPlaneAddresses {
    pub generation: u32,
    pub bridge_signer: [u8; 20],
    pub governance_operator: [u8; 20],
    pub runtime_administrator: [u8; 20],
    pub independent_canceller: [u8; 20],
}

#[derive(CandidType, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigningFailureClass {
    InsufficientCycles,
    CostUnavailable,
    CallRejected,
    CallFailed,
    ResponseDecode,
    InvalidPublicKey,
    InvalidSignature,
    RecoveryMismatch,
    Storage,
}

#[derive(Debug)]
pub enum SignerError {
    ManagementCall {
        operation: &'static str,
        class: SigningFailureClass,
        detail: String,
    },
    InvalidPublicKey,
    InvalidSignature,
    RecoveryFailed,
}

impl SignerError {
    pub fn class(&self) -> SigningFailureClass {
        match self {
            Self::ManagementCall { class, .. } => *class,
            Self::InvalidPublicKey => SigningFailureClass::InvalidPublicKey,
            Self::InvalidSignature => SigningFailureClass::InvalidSignature,
            Self::RecoveryFailed => SigningFailureClass::RecoveryMismatch,
        }
    }
}

impl std::fmt::Display for SignerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ManagementCall {
                operation,
                class,
                detail,
            } => write!(formatter, "{operation} {class:?}: {detail}"),
            Self::InvalidPublicKey => formatter.write_str("invalid public key"),
            Self::InvalidSignature => formatter.write_str("invalid signature"),
            Self::RecoveryFailed => formatter.write_str("signature recovery failed"),
        }
    }
}

fn management_error(operation: &'static str, error: ic_cdk::call::Error) -> SignerError {
    let class = match &error {
        ic_cdk::call::Error::InsufficientLiquidCycleBalance(_) => {
            SigningFailureClass::InsufficientCycles
        }
        ic_cdk::call::Error::CallPerformFailed(_) => SigningFailureClass::CallFailed,
        ic_cdk::call::Error::CallRejected(_) => SigningFailureClass::CallRejected,
        ic_cdk::call::Error::CandidDecodeFailed(_) => SigningFailureClass::ResponseDecode,
    };
    SignerError::ManagementCall {
        operation,
        class,
        detail: format!("{error:?}"),
    }
}

async fn bounded_management_call<A, R>(
    operation: &'static str,
    args: &A,
    cycles: u128,
) -> Result<R, SignerError>
where
    A: ArgumentEncoder,
    R: CandidType + DeserializeOwned,
{
    Call::bounded_wait(Principal::management_canister(), operation)
        .change_timeout(PUBLIC_KEY_CALL_TIMEOUT_SECONDS)
        .with_args(args)
        .with_cycles(cycles)
        .await
        .map_err(|error| management_error(operation, error.into()))?
        .candid()
        .map_err(|error| {
            management_error(operation, ic_cdk::call::Error::CandidDecodeFailed(error))
        })
}

pub async fn sign_governance_for_role(
    envelope: &GovernanceTransactionEnvelope,
    config: &BridgeInitArgs,
    role: SignerRole,
) -> Result<Vec<u8>, SignerError> {
    if role == SignerRole::Mint {
        return Err(SignerError::InvalidSignature);
    }
    sign_for_role(envelope, config, role).await
}

pub async fn sign_mint_authorization_digest(
    digest: [u8; 32],
    config: &BridgeInitArgs,
) -> Result<Vec<u8>, SignerError> {
    let generation = active_generation()?;
    let key_id = EcdsaKeyId {
        curve: EcdsaCurve::Secp256k1,
        name: config.ecdsa_key_name.clone(),
    };
    let public_key = signer_public_key(config, &key_id, SignerRole::Mint, generation).await?;
    let sign_args = SignWithEcdsaArgs {
        message_hash: digest.to_vec(),
        derivation_path: derivation_path(config, SignerRole::Mint, generation),
        key_id,
    };
    let raw_signature = threshold_signature(&sign_args, config).await?;
    canonical_ethereum_signature(digest, &public_key, &raw_signature)
}

async fn sign_for_role(
    envelope: &GovernanceTransactionEnvelope,
    config: &BridgeInitArgs,
    role: SignerRole,
) -> Result<Vec<u8>, SignerError> {
    let generation = active_generation()?;
    let unsigned = unsigned_transaction(envelope);
    let signing_hash = keccak(&unsigned);
    let key_id = EcdsaKeyId {
        curve: EcdsaCurve::Secp256k1,
        name: config.ecdsa_key_name.clone(),
    };
    let public_key = signer_public_key(config, &key_id, role, generation).await?;
    let sign_args = SignWithEcdsaArgs {
        message_hash: signing_hash.to_vec(),
        derivation_path: derivation_path(config, role, generation),
        key_id,
    };
    let raw_signature = threshold_signature(&sign_args, config).await?;
    assemble_signed(envelope, signing_hash, &public_key, &raw_signature)
}

async fn signer_public_key(
    config: &BridgeInitArgs,
    key_id: &EcdsaKeyId,
    role: SignerRole,
    generation: u32,
) -> Result<Vec<u8>, SignerError> {
    Ok(
        match STORE
            .with(|store| match role {
                SignerRole::Mint => store.borrow().signer_public_key(),
                SignerRole::Governance => store.borrow().governance_operator_public_key(),
                SignerRole::RuntimeAdministrator | SignerRole::Canceller => Ok(None),
            })
            .map_err(|error| SignerError::ManagementCall {
                operation: "read_cached_ecdsa_public_key",
                class: SigningFailureClass::Storage,
                detail: error.to_string(),
            })? {
            Some(public_key) => public_key,
            None => {
                let public_key = bounded_management_call::<_, EcdsaPublicKeyResult>(
                    "ecdsa_public_key",
                    &(&EcdsaPublicKeyArgs {
                        canister_id: None,
                        derivation_path: derivation_path(config, role, generation),
                        key_id: key_id.clone(),
                    },),
                    0,
                )
                .await?
                .public_key;
                VerifyingKey::from_sec1_bytes(&public_key)
                    .map_err(|_| SignerError::InvalidPublicKey)?;
                STORE
                    .with(|store| {
                        let mut store = store.borrow_mut();
                        match role {
                            SignerRole::Mint => store.set_signer_public_key_if_absent(public_key),
                            SignerRole::Governance => {
                                store.set_governance_operator_public_key_if_absent(public_key)
                            }
                            SignerRole::RuntimeAdministrator | SignerRole::Canceller => {
                                Ok(public_key)
                            }
                        }
                    })
                    .map_err(|error| SignerError::ManagementCall {
                        operation: "cache_ecdsa_public_key",
                        class: SigningFailureClass::Storage,
                        detail: error.to_string(),
                    })?
            }
        },
    )
}

async fn threshold_signature(
    sign_args: &SignWithEcdsaArgs,
    config: &BridgeInitArgs,
) -> Result<Vec<u8>, SignerError> {
    let cycles = cost_sign_with_ecdsa(sign_args).map_err(|error| SignerError::ManagementCall {
        operation: "sign_with_ecdsa",
        class: SigningFailureClass::CostUnavailable,
        detail: format!("{error:?}"),
    })?;
    let required_reserve = STORE.with(|store| {
        let store = store.borrow();
        let nonterminal_withdrawals = store
            .nonterminal_withdrawal_count()
            .map_err(signing_storage_error)?;
        let nonterminal_deposits = store
            .nonterminal_deposit_count()
            .map_err(signing_storage_error)?;
        let reserved_deposits = store
            .deposit_funding_reservation_count()
            .map_err(signing_storage_error)?;
        config
            .reserve_policy()
            .required_cycles(
                nonterminal_withdrawals,
                nonterminal_deposits,
                reserved_deposits,
            )
            .map_err(signing_storage_error)
    })?;
    let required_balance = bridge_core::signing_cycle_requirement(
        required_reserve,
        cycles,
        config.settlement_cycle_ceiling,
    )
    .ok_or_else(|| SignerError::ManagementCall {
        operation: "sign_with_ecdsa",
        class: SigningFailureClass::InsufficientCycles,
        detail: "signing cycle requirement overflow".to_string(),
    })?;
    let available = ic_cdk::api::canister_liquid_cycle_balance();
    if available < required_balance {
        return Err(SignerError::ManagementCall {
            operation: "sign_with_ecdsa",
            class: SigningFailureClass::InsufficientCycles,
            detail: format!("available={available} required={required_balance}"),
        });
    }
    let result = {
        ::ic_cdk_management_canister::sign_with_ecdsa(sign_args)
            .await
            .map_err(signing_call_error)?
    };
    Ok(result.signature)
}

fn signing_call_error(error: SignCallError) -> SignerError {
    let class = match &error {
        SignCallError::SignCostError(_) => SigningFailureClass::CostUnavailable,
        SignCallError::CallFailed(ic_cdk::call::CallFailed::InsufficientLiquidCycleBalance(_)) => {
            SigningFailureClass::InsufficientCycles
        }
        SignCallError::CallFailed(ic_cdk::call::CallFailed::CallPerformFailed(_)) => {
            SigningFailureClass::CallFailed
        }
        SignCallError::CallFailed(ic_cdk::call::CallFailed::CallRejected(_)) => {
            SigningFailureClass::CallRejected
        }
        SignCallError::CandidDecodeFailed(_) => SigningFailureClass::ResponseDecode,
    };
    SignerError::ManagementCall {
        operation: "sign_with_ecdsa",
        class,
        detail: format!("{error:?}"),
    }
}

fn signing_storage_error(error: impl std::fmt::Display) -> SignerError {
    SignerError::ManagementCall {
        operation: "sign_with_ecdsa_budget",
        class: SigningFailureClass::Storage,
        detail: error.to_string(),
    }
}

pub async fn ethereum_address(config: &BridgeInitArgs) -> Result<[u8; 20], SignerError> {
    ethereum_address_for_role(config, SignerRole::Mint).await
}

pub async fn governance_operator_address(config: &BridgeInitArgs) -> Result<[u8; 20], SignerError> {
    ethereum_address_for_role(config, SignerRole::Governance).await
}

pub async fn runtime_administrator_address(
    config: &BridgeInitArgs,
) -> Result<[u8; 20], SignerError> {
    ethereum_address_for_role(config, SignerRole::RuntimeAdministrator).await
}

pub async fn canceller_address(config: &BridgeInitArgs) -> Result<[u8; 20], SignerError> {
    ethereum_address_for_role(config, SignerRole::Canceller).await
}

async fn ethereum_address_for_role(
    config: &BridgeInitArgs,
    role: SignerRole,
) -> Result<[u8; 20], SignerError> {
    ethereum_address_for_role_at_generation(config, role, active_generation()?).await
}

async fn ethereum_address_for_role_at_generation(
    config: &BridgeInitArgs,
    role: SignerRole,
    generation: u32,
) -> Result<[u8; 20], SignerError> {
    let result = bounded_management_call::<_, EcdsaPublicKeyResult>(
        "ecdsa_public_key",
        &(&EcdsaPublicKeyArgs {
            canister_id: None,
            derivation_path: derivation_path(config, role, generation),
            key_id: EcdsaKeyId {
                curve: EcdsaCurve::Secp256k1,
                name: config.ecdsa_key_name.clone(),
            },
        },),
        0,
    )
    .await?;
    let key = VerifyingKey::from_sec1_bytes(&result.public_key)
        .map_err(|_| SignerError::InvalidPublicKey)?;
    Ok(ethereum_address_from_key(&key))
}

pub async fn control_plane_addresses_for_generation(
    config: &BridgeInitArgs,
    generation: u32,
) -> Result<ControlPlaneAddresses, SignerError> {
    let bridge_signer =
        ethereum_address_for_role_at_generation(config, SignerRole::Mint, generation).await?;
    let governance_operator =
        ethereum_address_for_role_at_generation(config, SignerRole::Governance, generation).await?;
    let runtime_administrator = ethereum_address_for_role_at_generation(
        config,
        SignerRole::RuntimeAdministrator,
        generation,
    )
    .await?;
    let independent_canceller =
        ethereum_address_for_role_at_generation(config, SignerRole::Canceller, generation).await?;
    Ok(ControlPlaneAddresses {
        generation,
        bridge_signer,
        governance_operator,
        runtime_administrator,
        independent_canceller,
    })
}

fn derivation_path(config: &BridgeInitArgs, role: SignerRole, generation: u32) -> Vec<Vec<u8>> {
    let path = match role {
        SignerRole::Mint => config.ecdsa_derivation_path.clone(),
        SignerRole::Governance => config.governance_ecdsa_derivation_path.clone(),
        SignerRole::RuntimeAdministrator | SignerRole::Canceller => {
            config.governance_ecdsa_derivation_path.clone()
        }
    };
    role_generation_derivation_path(path, role, generation)
}

fn role_generation_derivation_path(
    mut path: Vec<Vec<u8>>,
    role: SignerRole,
    generation: u32,
) -> Vec<Vec<u8>> {
    match role {
        SignerRole::Mint | SignerRole::Governance => {}
        SignerRole::RuntimeAdministrator => {
            path.push(b"KINIC-RUNTIME-ADMINISTRATOR-V1".to_vec());
        }
        SignerRole::Canceller => {
            path.push(b"KINIC-INDEPENDENT-CANCELLER-V1".to_vec());
        }
    }
    if generation != 0 {
        path.push(b"KINIC-CONTROL-PLANE-GENERATION-V1".to_vec());
        path.push(generation.to_be_bytes().to_vec());
    }
    path
}

fn active_generation() -> Result<u32, SignerError> {
    STORE
        .with(|store| store.borrow().control_plane_key_generation())
        .map_err(signing_storage_error)
}

fn ethereum_address_from_key(key: &VerifyingKey) -> [u8; 20] {
    let uncompressed = key.to_encoded_point(false);
    let hash = keccak(&uncompressed.as_bytes()[1..]);
    hash[12..].try_into().expect("Ethereum address")
}

fn recoverable_signature(
    signing_hash: [u8; 32],
    expected: &VerifyingKey,
    raw_signature: &[u8],
) -> Result<(Signature, RecoveryId), SignerError> {
    let mut signature =
        Signature::from_slice(raw_signature).map_err(|_| SignerError::InvalidSignature)?;
    if let Some(normalized) = signature.normalize_s() {
        signature = normalized;
    }
    for odd in [false, true] {
        let recovery = RecoveryId::new(odd, false);
        if VerifyingKey::recover_from_prehash(&signing_hash, &signature, recovery)
            .is_ok_and(|recovered| recovered == *expected)
        {
            return Ok((signature, recovery));
        }
    }
    Err(SignerError::RecoveryFailed)
}

fn canonical_ethereum_signature(
    signing_hash: [u8; 32],
    public_key: &[u8],
    raw_signature: &[u8],
) -> Result<Vec<u8>, SignerError> {
    let expected =
        VerifyingKey::from_sec1_bytes(public_key).map_err(|_| SignerError::InvalidPublicKey)?;
    let (signature, recovery) = recoverable_signature(signing_hash, &expected, raw_signature)?;
    let mut encoded = Vec::with_capacity(65);
    encoded.extend_from_slice(&signature.to_bytes());
    encoded.push(if recovery.is_y_odd() { 28 } else { 27 });
    Ok(encoded)
}

pub fn recover_ethereum_address(
    signing_hash: [u8; 32],
    signature: &[u8],
) -> Result<[u8; 20], SignerError> {
    if signature.len() != 65 {
        return Err(SignerError::InvalidSignature);
    }
    let (compact, recovery_byte) = signature.split_at(64);
    let signature = Signature::from_slice(compact).map_err(|_| SignerError::InvalidSignature)?;
    if signature.normalize_s().is_some() {
        return Err(SignerError::InvalidSignature);
    }
    let recovery = match recovery_byte[0] {
        27 => RecoveryId::new(false, false),
        28 => RecoveryId::new(true, false),
        _ => return Err(SignerError::InvalidSignature),
    };
    let recovered = VerifyingKey::recover_from_prehash(&signing_hash, &signature, recovery)
        .map_err(|_| SignerError::RecoveryFailed)?;
    Ok(ethereum_address_from_key(&recovered))
}

fn assemble_signed(
    envelope: &GovernanceTransactionEnvelope,
    signing_hash: [u8; 32],
    public_key: &[u8],
    raw_signature: &[u8],
) -> Result<Vec<u8>, SignerError> {
    let expected =
        VerifyingKey::from_sec1_bytes(public_key).map_err(|_| SignerError::InvalidPublicKey)?;
    let (signature, recovery) = recoverable_signature(signing_hash, &expected, raw_signature)?;
    let parity = recovery.is_y_odd();
    let bytes = signature.to_bytes();
    let r = trim_integer(&bytes[..32]);
    let s = trim_integer(&bytes[32..]);
    let payload = rlp_list(&[
        rlp_u64(envelope.chain_id),
        rlp_u64(envelope.nonce),
        rlp_u128(envelope.max_priority_fee_per_gas),
        rlp_u128(envelope.max_fee_per_gas),
        rlp_u128(envelope.gas_limit),
        rlp_bytes(&envelope.contract),
        rlp_bytes(&[]),
        rlp_bytes(&envelope.calldata),
        rlp_list(&[]),
        rlp_u64(u64::from(parity)),
        rlp_bytes(r),
        rlp_bytes(s),
    ]);
    let mut transaction = vec![0x02];
    transaction.extend(payload);
    Ok(transaction)
}

fn unsigned_transaction(envelope: &GovernanceTransactionEnvelope) -> Vec<u8> {
    let payload = rlp_list(&[
        rlp_u64(envelope.chain_id),
        rlp_u64(envelope.nonce),
        rlp_u128(envelope.max_priority_fee_per_gas),
        rlp_u128(envelope.max_fee_per_gas),
        rlp_u128(envelope.gas_limit),
        rlp_bytes(&envelope.contract),
        rlp_bytes(&[]),
        rlp_bytes(&envelope.calldata),
        rlp_list(&[]),
    ]);
    let mut transaction = vec![0x02];
    transaction.extend(payload);
    transaction
}

fn keccak(bytes: &[u8]) -> [u8; 32] {
    let mut result = [0u8; 32];
    let mut hash = Keccak::v256();
    hash.update(bytes);
    hash.finalize(&mut result);
    result
}

fn trim_integer(bytes: &[u8]) -> &[u8] {
    let first = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    &bytes[first..]
}

fn rlp_u64(value: u64) -> Vec<u8> {
    rlp_bytes(trim_integer(&value.to_be_bytes()))
}

fn rlp_u128(value: u128) -> Vec<u8> {
    rlp_bytes(trim_integer(&value.to_be_bytes()))
}

fn rlp_bytes(value: &[u8]) -> Vec<u8> {
    if value.len() == 1 && value[0] < 0x80 {
        return value.to_vec();
    }
    let mut result = length_prefix(value.len(), 0x80, 0xb7);
    result.extend_from_slice(value);
    result
}

fn rlp_list(items: &[Vec<u8>]) -> Vec<u8> {
    let length = items.iter().map(Vec::len).sum();
    let mut result = length_prefix(length, 0xc0, 0xf7);
    for item in items {
        result.extend_from_slice(item);
    }
    result
}

fn length_prefix(length: usize, short_offset: u8, long_offset: u8) -> Vec<u8> {
    if length <= 55 {
        return vec![short_offset + length as u8];
    }
    let length_bytes = length.to_be_bytes();
    let bytes = trim_integer(&length_bytes);
    let mut result = vec![long_offset + bytes.len() as u8];
    result.extend_from_slice(bytes);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_key_lookup_keeps_the_fixed_sixty_second_bound() {
        assert_eq!(PUBLIC_KEY_CALL_TIMEOUT_SECONDS, 60);
    }

    #[test]
    fn signer_errors_expose_only_the_safe_failure_class() {
        assert_eq!(
            SignerError::InvalidPublicKey.class(),
            SigningFailureClass::InvalidPublicKey
        );
        assert_eq!(
            SignerError::InvalidSignature.class(),
            SigningFailureClass::InvalidSignature
        );
        assert_eq!(
            SignerError::RecoveryFailed.class(),
            SigningFailureClass::RecoveryMismatch
        );
    }

    #[test]
    fn control_plane_derivation_paths_are_role_and_generation_separated() {
        let mint_zero =
            role_generation_derivation_path(vec![b"mint".to_vec()], SignerRole::Mint, 0);
        let governance_zero = role_generation_derivation_path(
            vec![b"governance".to_vec()],
            SignerRole::Governance,
            0,
        );
        let runtime_zero = role_generation_derivation_path(
            vec![b"governance".to_vec()],
            SignerRole::RuntimeAdministrator,
            0,
        );
        let canceller_zero =
            role_generation_derivation_path(vec![b"governance".to_vec()], SignerRole::Canceller, 0);
        let generation_one =
            |role| role_generation_derivation_path(vec![b"governance".to_vec()], role, 1);

        assert_ne!(mint_zero, governance_zero);
        assert_ne!(governance_zero, runtime_zero);
        assert_ne!(governance_zero, canceller_zero);
        assert_ne!(runtime_zero, canceller_zero);
        for role in [
            SignerRole::Mint,
            SignerRole::Governance,
            SignerRole::RuntimeAdministrator,
            SignerRole::Canceller,
        ] {
            let path = generation_one(role);
            assert_eq!(path[path.len() - 2], b"KINIC-CONTROL-PLANE-GENERATION-V1");
            assert_eq!(path[path.len() - 1], 1u32.to_be_bytes());
        }
    }
    use bridge_core::GovernanceOperationId;

    #[test]
    fn unsigned_eip1559_encoding_is_stable_and_hashable() {
        let envelope = GovernanceTransactionEnvelope {
            operation_id: GovernanceOperationId::new(1),
            payload_hash: [2; 32],
            nonce: 0,
            chain_id: 8453,
            contract: [3; 20],
            calldata: vec![1, 2, 3, 4],
            gas_limit: 100_000,
            max_fee_per_gas: 20,
            max_priority_fee_per_gas: 1,
            signed_transactions: vec![],
        };
        let encoded = unsigned_transaction(&envelope);
        assert_eq!(encoded[0], 2);
        assert_ne!(keccak(&encoded), [0; 32]);
        assert_eq!(unsigned_transaction(&envelope), encoded);
    }
}
