use crate::{config::BridgeInitArgs, STORE};
use bridge_core::EvmTransactionEnvelope;
use candid::{utils::ArgumentEncoder, CandidType, Principal};
use ic_cdk::call::Call;
use ic_cdk_management_canister::{
    cost_sign_with_ecdsa, EcdsaCurve, EcdsaKeyId, EcdsaPublicKeyArgs, EcdsaPublicKeyResult,
    SignWithEcdsaArgs, SignWithEcdsaResult,
};
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

const SIGNING_CALL_TIMEOUT_SECONDS: u32 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignerRole {
    Mint,
    Governance,
}

#[derive(Debug)]
pub enum SignerError {
    ManagementCall {
        operation: &'static str,
        class: &'static str,
        detail: String,
    },
    InvalidPublicKey,
    InvalidSignature,
    RecoveryFailed,
}

impl std::fmt::Display for SignerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ManagementCall {
                operation,
                class,
                detail,
            } => write!(formatter, "{operation} {class}: {detail}"),
            Self::InvalidPublicKey => formatter.write_str("invalid public key"),
            Self::InvalidSignature => formatter.write_str("invalid signature"),
            Self::RecoveryFailed => formatter.write_str("signature recovery failed"),
        }
    }
}

fn management_error(operation: &'static str, error: ic_cdk::call::Error) -> SignerError {
    let class = match &error {
        ic_cdk::call::Error::InsufficientLiquidCycleBalance(_) => "insufficient_cycles",
        ic_cdk::call::Error::CallPerformFailed(_) => "call_perform",
        ic_cdk::call::Error::CallRejected(_) => "rejected_or_timeout",
        ic_cdk::call::Error::CandidDecodeFailed(_) => "candid_decode",
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
        .change_timeout(SIGNING_CALL_TIMEOUT_SECONDS)
        .with_args(args)
        .with_cycles(cycles)
        .await
        .map_err(|error| management_error(operation, error.into()))?
        .candid()
        .map_err(|error| {
            management_error(operation, ic_cdk::call::Error::CandidDecodeFailed(error))
        })
}

pub async fn sign(
    envelope: &EvmTransactionEnvelope,
    config: &BridgeInitArgs,
) -> Result<Vec<u8>, SignerError> {
    sign_for_role(envelope, config, SignerRole::Mint).await
}

pub async fn sign_governance(
    envelope: &EvmTransactionEnvelope,
    config: &BridgeInitArgs,
) -> Result<Vec<u8>, SignerError> {
    sign_for_role(envelope, config, SignerRole::Governance).await
}

async fn sign_for_role(
    envelope: &EvmTransactionEnvelope,
    config: &BridgeInitArgs,
    role: SignerRole,
) -> Result<Vec<u8>, SignerError> {
    let unsigned = unsigned_transaction(envelope);
    let signing_hash = keccak(&unsigned);
    let key_id = EcdsaKeyId {
        curve: EcdsaCurve::Secp256k1,
        name: config.ecdsa_key_name.clone(),
    };
    let public_key = signer_public_key(config, &key_id, role).await?;
    let sign_args = SignWithEcdsaArgs {
        message_hash: signing_hash.to_vec(),
        derivation_path: derivation_path(config, role).to_vec(),
        key_id,
    };
    let raw_signature = threshold_signature(&sign_args).await?;
    assemble_signed(envelope, signing_hash, &public_key, &raw_signature)
}

async fn signer_public_key(
    config: &BridgeInitArgs,
    key_id: &EcdsaKeyId,
    role: SignerRole,
) -> Result<Vec<u8>, SignerError> {
    Ok(
        match STORE
            .with(|store| match role {
                SignerRole::Mint => store.borrow().signer_public_key(),
                SignerRole::Governance => store.borrow().governance_operator_public_key(),
            })
            .map_err(|error| SignerError::ManagementCall {
                operation: "read_cached_ecdsa_public_key",
                class: "storage",
                detail: error.to_string(),
            })? {
            Some(public_key) => public_key,
            None => {
                let public_key = bounded_management_call::<_, EcdsaPublicKeyResult>(
                    "ecdsa_public_key",
                    &(&EcdsaPublicKeyArgs {
                        canister_id: None,
                        derivation_path: derivation_path(config, role).to_vec(),
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
                        }
                    })
                    .map_err(|error| SignerError::ManagementCall {
                        operation: "cache_ecdsa_public_key",
                        class: "storage",
                        detail: error.to_string(),
                    })?
            }
        },
    )
}

async fn threshold_signature(sign_args: &SignWithEcdsaArgs) -> Result<Vec<u8>, SignerError> {
    let cycles = cost_sign_with_ecdsa(sign_args).map_err(|error| SignerError::ManagementCall {
        operation: "sign_with_ecdsa",
        class: "sign_cost",
        detail: format!("{error:?}"),
    })?;
    Ok(
        bounded_management_call::<_, SignWithEcdsaResult>("sign_with_ecdsa", &(sign_args,), cycles)
            .await?
            .signature,
    )
}

/// Signs only the release/address-bound control challenge understood by bridge-profile.
/// The caller cannot supply a message hash, which keeps this endpoint from becoming a
/// general-purpose threshold-ECDSA signing oracle.
pub async fn sign_chain_key_challenge(
    release_id: &str,
    config: &BridgeInitArgs,
) -> Result<String, SignerError> {
    let key_id = EcdsaKeyId {
        curve: EcdsaCurve::Secp256k1,
        name: config.ecdsa_key_name.clone(),
    };
    let public_key = signer_public_key(config, &key_id, SignerRole::Mint).await?;
    let expected =
        VerifyingKey::from_sec1_bytes(&public_key).map_err(|_| SignerError::InvalidPublicKey)?;
    let address = ethereum_address_from_key(&expected);
    let challenge_hash = chain_key_challenge_hash(release_id, address);
    let signing_hash = eip191_hash(challenge_hash);
    let sign_args = SignWithEcdsaArgs {
        message_hash: signing_hash.to_vec(),
        derivation_path: config.ecdsa_derivation_path.clone(),
        key_id,
    };
    let raw_signature = threshold_signature(&sign_args).await?;
    let (signature, recovery) = recoverable_signature(signing_hash, &expected, &raw_signature)?;
    Ok(ethereum_signature_hex(&signature, recovery))
}

fn ethereum_signature_hex(signature: &Signature, recovery: RecoveryId) -> String {
    let mut bytes = signature.to_bytes().to_vec();
    bytes.push(27 + u8::from(recovery.is_y_odd()));
    format!("0x{}", encode_hex(&bytes))
}

pub async fn ethereum_address(config: &BridgeInitArgs) -> Result<[u8; 20], SignerError> {
    ethereum_address_for_role(config, SignerRole::Mint).await
}

pub async fn governance_operator_address(config: &BridgeInitArgs) -> Result<[u8; 20], SignerError> {
    ethereum_address_for_role(config, SignerRole::Governance).await
}

async fn ethereum_address_for_role(
    config: &BridgeInitArgs,
    role: SignerRole,
) -> Result<[u8; 20], SignerError> {
    let result = bounded_management_call::<_, EcdsaPublicKeyResult>(
        "ecdsa_public_key",
        &(&EcdsaPublicKeyArgs {
            canister_id: None,
            derivation_path: derivation_path(config, role).to_vec(),
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

fn derivation_path(config: &BridgeInitArgs, role: SignerRole) -> &[Vec<u8>] {
    match role {
        SignerRole::Mint => &config.ecdsa_derivation_path,
        SignerRole::Governance => &config.governance_ecdsa_derivation_path,
    }
}

fn ethereum_address_from_key(key: &VerifyingKey) -> [u8; 20] {
    let uncompressed = key.to_encoded_point(false);
    let hash = keccak(&uncompressed.as_bytes()[1..]);
    hash[12..].try_into().expect("Ethereum address")
}

fn chain_key_challenge_hash(release_id: &str, address: [u8; 20]) -> [u8; 32] {
    Sha256::digest(
        format!(
            "KINIC Bridge chain-key control v1\nrelease_id={release_id}\naddress=0x{}",
            encode_hex(&address)
        )
        .as_bytes(),
    )
    .into()
}

fn eip191_hash(message_hash: [u8; 32]) -> [u8; 32] {
    let mut payload = Vec::with_capacity(60);
    payload.extend_from_slice(b"\x19Ethereum Signed Message:\n32");
    payload.extend_from_slice(&message_hash);
    keccak(&payload)
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

fn assemble_signed(
    envelope: &EvmTransactionEnvelope,
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

fn unsigned_transaction(envelope: &EvmTransactionEnvelope) -> Vec<u8> {
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

pub fn transaction_hash(raw: &[u8]) -> [u8; 32] {
    keccak(raw)
}

fn keccak(bytes: &[u8]) -> [u8; 32] {
    let mut result = [0u8; 32];
    let mut hash = Keccak::v256();
    hash.update(bytes);
    hash.finalize(&mut result);
    result
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
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
    use k256::ecdsa::{signature::hazmat::PrehashSigner, SigningKey};

    #[test]
    fn threshold_signing_uses_the_fixed_sixty_second_bound() {
        assert_eq!(SIGNING_CALL_TIMEOUT_SECONDS, 60);
    }
    use bridge_core::EvmOperationId;

    #[test]
    fn unsigned_eip1559_encoding_is_stable_and_hashable() {
        let envelope = EvmTransactionEnvelope {
            operation_id: EvmOperationId::new(1),
            payload_hash: [2; 32],
            nonce: 0,
            chain_id: 8453,
            contract: [3; 20],
            calldata: vec![1, 2, 3, 4],
            gas_limit: 100_000,
            max_fee_per_gas: 20,
            max_priority_fee_per_gas: 1,
            initial_max_fee_per_gas: 20,
            initial_max_priority_fee_per_gas: 1,
            replacement_generation: 0,
            prior_signed_transactions: vec![],
            first_broadcast_at_ns: 0,
            last_broadcast_at_ns: 0,
            rebroadcast_count: 0,
            signed_transaction: None,
        };
        let encoded = unsigned_transaction(&envelope);
        assert_eq!(encoded[0], 2);
        assert_ne!(transaction_hash(&encoded), [0; 32]);
        assert_eq!(unsigned_transaction(&envelope), encoded);
    }

    #[test]
    fn chain_key_challenge_is_release_and_lowercase_address_bound() {
        assert_eq!(
            encode_hex(&chain_key_challenge_hash("release-1", [0x11; 20])),
            "2cab8b92b1a770f804a3e1956f86f42c41c6102e3479c0d7e6e01b982ee72ee3"
        );
    }

    #[test]
    fn challenge_signature_recovery_selects_the_chain_key() {
        let key = SigningKey::from_bytes((&[7_u8; 32]).into()).expect("test key");
        let expected = key.verifying_key();
        let digest = eip191_hash(chain_key_challenge_hash("release-1", [0x11; 20]));
        let raw: Signature = key.sign_prehash(&digest).expect("sign challenge");
        let (signature, recovery) =
            recoverable_signature(digest, expected, &raw.to_bytes()).expect("recover key");
        let recovered = VerifyingKey::recover_from_prehash(&digest, &signature, recovery)
            .expect("recover signature");
        assert_eq!(&recovered, expected);
        let encoded = ethereum_signature_hex(&signature, recovery);
        assert_eq!(encoded.len(), 2 + 65 * 2);
        assert!(encoded.ends_with("1b") || encoded.ends_with("1c"));
    }
}
