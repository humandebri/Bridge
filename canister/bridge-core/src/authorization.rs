use crate::Amount;

pub const MINT_AUTHORIZATION_TTL_SECONDS: u64 = 7_200;
pub const MINT_AUTHORIZATION_DOMAIN_NAME: &str = "KINIC Bridge";
pub const MINT_AUTHORIZATION_DOMAIN_VERSION: &str = "1";

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MintAuthorizationDomain {
    pub name: String,
    pub version: String,
    pub chain_id: u64,
    pub verifying_contract: [u8; 20],
}

impl MintAuthorizationDomain {
    pub fn bridge(chain_id: u64, verifying_contract: [u8; 20]) -> Self {
        Self {
            name: MINT_AUTHORIZATION_DOMAIN_NAME.into(),
            version: MINT_AUTHORIZATION_DOMAIN_VERSION.into(),
            chain_id,
            verifying_contract,
        }
    }
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MintAuthorization {
    pub deposit_id: [u8; 32],
    pub recipient: [u8; 20],
    pub gross_amount: Amount,
    pub max_service_fee: Amount,
    pub charged_service_fee: Amount,
    pub deadline: u64,
    pub authorization_epoch: u64,
}

impl MintAuthorization {
    pub fn deadline_from_finalized_timestamp(finalized_timestamp: u64) -> Option<u64> {
        finalized_timestamp.checked_add(MINT_AUTHORIZATION_TTL_SECONDS)
    }
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MintAuthorizationOrigin {
    pub finalized_block_number: u64,
    pub finalized_block_hash: [u8; 32],
    pub finalized_block_timestamp: u64,
}

/// Durable proof that an authorization can no longer be accepted on Base.
///
/// The current signer and epoch are observations, not equality requirements:
/// pausing or rotating the signer may invalidate an authorization early, but
/// refund still waits for its original deadline and a finalized unprocessed
/// observation.
#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MintExpiryEvidence {
    pub deposit_id: [u8; 32],
    pub authorization_digest: [u8; 32],
    pub chain_id: u64,
    pub verifying_contract: [u8; 20],
    pub deposit_processed: bool,
    pub finalized_block_number: u64,
    pub finalized_block_hash: [u8; 32],
    pub finalized_block_timestamp: u64,
    pub bridge_signer: [u8; 20],
    pub mint_authorization_epoch: u64,
    pub runtime_sha256: [u8; 32],
    pub rpc_request_digest: [u8; 32],
    pub rpc_response_digest: [u8; 32],
}

/// Durable proof binding a terminal `Minted` state to one exact authorization,
/// canonical receipt, and finalized Base head.
#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MintFinalizationEvidence {
    pub deposit_id: [u8; 32],
    pub recipient: [u8; 20],
    pub authorization_digest: [u8; 32],
    pub chain_id: u64,
    pub verifying_contract: [u8; 20],
    pub gross_amount: Amount,
    pub charged_service_fee: Amount,
    pub minted_amount: Amount,
    pub transaction_hash: [u8; 32],
    pub log_index: u64,
    pub receipt_succeeded: bool,
    pub receipt_block_number: u64,
    pub receipt_block_hash: [u8; 32],
    pub finalized_block_number: u64,
    pub finalized_block_hash: [u8; 32],
    pub rpc_request_digest: [u8; 32],
    pub rpc_response_digest: [u8; 32],
}

#[cfg_attr(
    feature = "storage-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MintAuthorizationRecord {
    pub authorization: MintAuthorization,
    pub domain: MintAuthorizationDomain,
    pub digest: [u8; 32],
    pub origin: MintAuthorizationOrigin,
    pub signature_dispatch_attempt: u32,
    pub signature_dispatched: bool,
    /// Canonical low-s `r || s || v`, present only after signer verification.
    pub signature: Option<Vec<u8>>,
}

impl MintAuthorizationRecord {
    pub fn dispatch_signature(&mut self) -> Option<u32> {
        if self.signature.is_some() {
            return None;
        }
        self.signature_dispatch_attempt = self.signature_dispatch_attempt.checked_add(1)?;
        self.signature_dispatched = true;
        Some(self.signature_dispatch_attempt)
    }

    pub fn install_signature(&mut self, signature: Vec<u8>) -> bool {
        if signature.len() != 65 || self.signature.is_some() || !self.signature_dispatched {
            return false;
        }
        self.signature = Some(signature);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadline_is_fixed_and_checked() {
        assert_eq!(MINT_AUTHORIZATION_TTL_SECONDS, 7_200);
        assert_eq!(
            MintAuthorization::deadline_from_finalized_timestamp(10),
            Some(7_210)
        );
        assert_eq!(
            MintAuthorization::deadline_from_finalized_timestamp(u64::MAX),
            None
        );
    }

    #[test]
    fn signature_dispatch_reuses_record_and_never_replaces_signature() {
        let mut record = MintAuthorizationRecord {
            authorization: MintAuthorization {
                deposit_id: [1; 32],
                recipient: [2; 20],
                gross_amount: Amount::new(100),
                max_service_fee: Amount::new(10),
                charged_service_fee: Amount::new(10),
                deadline: 1_800,
                authorization_epoch: 1,
            },
            domain: MintAuthorizationDomain::bridge(8453, [3; 20]),
            digest: [4; 32],
            origin: MintAuthorizationOrigin {
                finalized_block_number: 7,
                finalized_block_hash: [5; 32],
                finalized_block_timestamp: 0,
            },
            signature_dispatch_attempt: 0,
            signature_dispatched: false,
            signature: None,
        };

        assert_eq!(record.dispatch_signature(), Some(1));
        assert_eq!(record.dispatch_signature(), Some(2));
        assert!(!record.install_signature(vec![0; 64]));
        assert!(record.install_signature(vec![0; 65]));
        assert_eq!(record.dispatch_signature(), None);
        assert!(!record.install_signature(vec![1; 65]));
    }
}
