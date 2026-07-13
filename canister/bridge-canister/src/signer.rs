use crate::config::BridgeInitArgs;
use bridge_core::EvmTransactionEnvelope;
use ic_cdk_management_canister::{
    ecdsa_public_key, sign_with_ecdsa, EcdsaCurve, EcdsaKeyId, EcdsaPublicKeyArgs,
    SignWithEcdsaArgs,
};
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use tiny_keccak::{Hasher, Keccak};

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

fn public_key_error(error: ic_cdk::call::Error) -> SignerError {
    let class = match &error {
        ic_cdk::call::Error::InsufficientLiquidCycleBalance(_) => "insufficient_cycles",
        ic_cdk::call::Error::CallPerformFailed(_) => "call_perform",
        ic_cdk::call::Error::CallRejected(_) => "rejected",
        ic_cdk::call::Error::CandidDecodeFailed(_) => "candid_decode",
    };
    SignerError::ManagementCall {
        operation: "ecdsa_public_key",
        class,
        detail: format!("{error:?}"),
    }
}

fn signature_error(error: ic_cdk_management_canister::SignCallError) -> SignerError {
    let class = match &error {
        ic_cdk_management_canister::SignCallError::SignCostError(_) => "sign_cost",
        ic_cdk_management_canister::SignCallError::CallFailed(_) => "call_failed",
        ic_cdk_management_canister::SignCallError::CandidDecodeFailed(_) => "candid_decode",
    };
    SignerError::ManagementCall {
        operation: "sign_with_ecdsa",
        class,
        detail: format!("{error:?}"),
    }
}

pub async fn sign(
    envelope: &EvmTransactionEnvelope,
    config: &BridgeInitArgs,
) -> Result<Vec<u8>, SignerError> {
    let unsigned = unsigned_transaction(envelope);
    let signing_hash = keccak(&unsigned);
    let key_id = EcdsaKeyId {
        curve: EcdsaCurve::Secp256k1,
        name: config.ecdsa_key_name.clone(),
    };
    let public_key = ecdsa_public_key(&EcdsaPublicKeyArgs {
        canister_id: None,
        derivation_path: config.ecdsa_derivation_path.clone(),
        key_id: key_id.clone(),
    })
    .await
    .map_err(public_key_error)?
    .public_key;
    let raw_signature = sign_with_ecdsa(&SignWithEcdsaArgs {
        message_hash: signing_hash.to_vec(),
        derivation_path: config.ecdsa_derivation_path.clone(),
        key_id,
    })
    .await
    .map_err(signature_error)?
    .signature;
    assemble_signed(envelope, signing_hash, &public_key, &raw_signature)
}

pub async fn ethereum_address(config: &BridgeInitArgs) -> Result<[u8; 20], SignerError> {
    let result = ecdsa_public_key(&EcdsaPublicKeyArgs {
        canister_id: None,
        derivation_path: config.ecdsa_derivation_path.clone(),
        key_id: EcdsaKeyId {
            curve: EcdsaCurve::Secp256k1,
            name: config.ecdsa_key_name.clone(),
        },
    })
    .await
    .map_err(public_key_error)?;
    let key = VerifyingKey::from_sec1_bytes(&result.public_key)
        .map_err(|_| SignerError::InvalidPublicKey)?;
    let uncompressed = key.to_encoded_point(false);
    let hash = keccak(&uncompressed.as_bytes()[1..]);
    Ok(hash[12..].try_into().expect("Ethereum address"))
}

fn assemble_signed(
    envelope: &EvmTransactionEnvelope,
    signing_hash: [u8; 32],
    public_key: &[u8],
    raw_signature: &[u8],
) -> Result<Vec<u8>, SignerError> {
    let expected =
        VerifyingKey::from_sec1_bytes(public_key).map_err(|_| SignerError::InvalidPublicKey)?;
    let mut signature =
        Signature::from_slice(raw_signature).map_err(|_| SignerError::InvalidSignature)?;
    if let Some(normalized) = signature.normalize_s() {
        signature = normalized;
    }
    let mut parity = None;
    for odd in [false, true] {
        let recovery = RecoveryId::new(odd, false);
        if let Ok(recovered) =
            VerifyingKey::recover_from_prehash(&signing_hash, &signature, recovery)
        {
            if recovered == expected {
                parity = Some(odd);
                break;
            }
        }
    }
    let parity = parity.ok_or(SignerError::RecoveryFailed)?;
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
            signed_transaction: None,
        };
        let encoded = unsigned_transaction(&envelope);
        assert_eq!(encoded[0], 2);
        assert_ne!(transaction_hash(&encoded), [0; 32]);
        assert_eq!(unsigned_transaction(&envelope), encoded);
    }
}
