use bridge_core::{MintAuthorization, MintAuthorizationDomain};
use tiny_keccak::{Hasher, Keccak};

const DOMAIN_TYPE: &[u8] =
    b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";
const AUTHORIZATION_TYPE: &[u8] = b"MintAuthorization(bytes32 depositId,address recipient,uint256 grossAmount,uint256 maxServiceFee,uint256 chargedServiceFee,uint256 deadline,uint256 authorizationEpoch)";

pub(crate) fn digest(
    domain: &MintAuthorizationDomain,
    authorization: MintAuthorization,
) -> [u8; 32] {
    let domain_separator = keccak(&concat_words(&[
        keccak(DOMAIN_TYPE),
        keccak(domain.name.as_bytes()),
        keccak(domain.version.as_bytes()),
        uint_word(u128::from(domain.chain_id)),
        address_word(domain.verifying_contract),
    ]));
    let struct_hash = keccak(&concat_words(&[
        keccak(AUTHORIZATION_TYPE),
        authorization.deposit_id,
        address_word(authorization.recipient),
        uint_word(authorization.gross_amount.get()),
        uint_word(authorization.max_service_fee.get()),
        uint_word(authorization.charged_service_fee.get()),
        uint_word(u128::from(authorization.deadline)),
        uint_word(u128::from(authorization.authorization_epoch)),
    ]));

    let mut input = Vec::with_capacity(66);
    input.extend_from_slice(&[0x19, 0x01]);
    input.extend_from_slice(&domain_separator);
    input.extend_from_slice(&struct_hash);
    keccak(&input)
}

fn concat_words(words: &[[u8; 32]]) -> Vec<u8> {
    words.concat()
}

fn uint_word(value: u128) -> [u8; 32] {
    let mut word = [0; 32];
    word[16..].copy_from_slice(&value.to_be_bytes());
    word
}

fn address_word(address: [u8; 20]) -> [u8; 32] {
    let mut word = [0; 32];
    word[12..].copy_from_slice(&address);
    word
}

fn keccak(bytes: &[u8]) -> [u8; 32] {
    let mut result = [0; 32];
    let mut hasher = Keccak::v256();
    hasher.update(bytes);
    hasher.finalize(&mut result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::{Amount, MintAuthorizationDomain};

    #[test]
    fn digest_is_bound_to_chain_contract_and_every_field() {
        let authorization = MintAuthorization {
            deposit_id: [1; 32],
            recipient: [2; 20],
            gross_amount: Amount::new(100),
            max_service_fee: Amount::new(10),
            charged_service_fee: Amount::new(9),
            deadline: 1_800,
            authorization_epoch: 1,
        };
        let domain = MintAuthorizationDomain::bridge(8453, [3; 20]);
        let expected = digest(&domain, authorization);

        let mut changed = authorization;
        changed.deadline += 1;
        assert_ne!(expected, digest(&domain, changed));
        assert_ne!(
            expected,
            digest(
                &MintAuthorizationDomain::bridge(8454, [3; 20]),
                authorization
            )
        );
        assert_ne!(
            expected,
            digest(
                &MintAuthorizationDomain::bridge(8453, [4; 20]),
                authorization
            )
        );
    }

    #[test]
    fn shared_protocol_vector_matches_digest_signature_and_signer() {
        let vector: serde_json::Value = serde_json::from_str(include_str!(
            "../../../verification/generated/mint-authorization-vector.json"
        ))
        .expect("valid vector");
        let domain = MintAuthorizationDomain::bridge(
            8453,
            hex_array::<20>(vector["domain"]["verifying_contract"].as_str().unwrap()),
        );
        let authorization = MintAuthorization {
            deposit_id: hex_array::<32>(vector["authorization"]["deposit_id"].as_str().unwrap()),
            recipient: hex_array::<20>(vector["authorization"]["recipient"].as_str().unwrap()),
            gross_amount: Amount::new(1_100),
            max_service_fee: Amount::new(100),
            charged_service_fee: Amount::new(10),
            deadline: 1_800_000_000,
            authorization_epoch: 7,
        };
        let expected_digest = hex_array::<32>(vector["digest"].as_str().unwrap());
        assert_eq!(digest(&domain, authorization), expected_digest);
        let signature = hex_bytes(vector["signature"].as_str().unwrap());
        assert_eq!(
            crate::signer::recover_ethereum_address(expected_digest, &signature).unwrap(),
            hex_array::<20>(vector["signer"].as_str().unwrap())
        );
    }

    fn hex_array<const N: usize>(value: &str) -> [u8; N] {
        hex_bytes(value).try_into().expect("fixed hex length")
    }

    fn hex_bytes(value: &str) -> Vec<u8> {
        value
            .strip_prefix("0x")
            .expect("0x prefix")
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).expect("hex byte")
            })
            .collect()
    }
}
