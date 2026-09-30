use crate::config::{AssetConfig, AssetLifecycle, BaseBridgeKind, BridgeInitArgs, KINIC_ASSET_ID};
use bridge_core::{Amount, MintAuthorization, MintAuthorizationDomain};
use candid::{CandidType, Deserialize, Principal};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

pub const SHARED_DOMAIN_NAME: &str = bridge_core::SHARED_MINT_AUTHORIZATION_DOMAIN_NAME;
pub const SHARED_DOMAIN_VERSION: &str = "1";

const DOMAIN_TYPE: &[u8] =
    b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";
const AUTHORIZATION_TYPE: &[u8] = b"MintAuthorization(bytes32 assetId,bytes32 depositId,address recipient,uint256 grossAmount,uint256 maxServiceFee,uint256 chargedServiceFee,uint256 deadline,uint256 globalEpoch,uint256 assetEpoch)";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedAuthorizationBinding {
    pub asset_id: [u8; 32],
    pub global_epoch: u64,
    pub asset_epoch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedMintAuthorization {
    pub asset_id: [u8; 32],
    pub authorization: MintAuthorization,
    pub global_epoch: u64,
    pub asset_epoch: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct AssetOperationArgs<T> {
    pub asset_id: Vec<u8>,
    pub operation: T,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetExecutionContext {
    pub root: BridgeInitArgs,
    pub asset: AssetConfig,
    pub asset_id: [u8; 32],
    pub bridge_contract: [u8; 20],
    pub token_contract: Option<[u8; 20]>,
    pub ledger_fee: Amount,
    pub custody_subaccount: [u8; 32],
}

impl AssetExecutionContext {
    pub fn new(
        root: BridgeInitArgs,
        asset: AssetConfig,
        require_enabled: bool,
    ) -> Result<Self, &'static str> {
        asset.validate_asset()?;
        if require_enabled && asset.lifecycle != AssetLifecycle::Enabled {
            return Err("asset is not enabled");
        }
        let asset_id: [u8; 32] = asset
            .asset_id
            .as_slice()
            .try_into()
            .map_err(|_| "asset ID must be 32 bytes")?;
        let bridge_contract = asset
            .bridge_contract
            .as_slice()
            .try_into()
            .map_err(|_| "bridge address must be 20 bytes")?;
        let token_contract = match asset.bridge_kind {
            BaseBridgeKind::LegacySingleToken => None,
            BaseBridgeKind::SharedMultiToken => Some(
                asset
                    .token_contract
                    .as_slice()
                    .try_into()
                    .map_err(|_| "token address must be 20 bytes")?,
            ),
        };
        let custody_subaccount = custody_subaccount(asset_id);
        Ok(Self {
            ledger_fee: Amount::new(asset.ledger_fee),
            root,
            asset,
            asset_id,
            bridge_contract,
            token_contract,
            custody_subaccount,
        })
    }

    pub fn ledger_canister_id(&self) -> Principal {
        self.asset.ledger_canister_id
    }

    pub fn is_shared(&self) -> bool {
        self.asset.bridge_kind == BaseBridgeKind::SharedMultiToken
    }

    pub fn evm_args(&self) -> BridgeInitArgs {
        let mut args = self.root.clone();
        args.ledger_canister_id = self.asset.ledger_canister_id;
        args.index_canister_id = self.asset.index_canister_id;
        args.base_chain_id = self.asset.base_chain_id;
        args.bridge_contract = self.asset.bridge_contract.clone();
        args.expected_bridge_runtime_sha256 = self.asset.expected_bridge_runtime_sha256.clone();
        args.timelock_contract = self.asset.timelock_contract.clone();
        args.expected_bsns_runtime_sha256 = self.asset.expected_token_runtime_sha256.clone();
        args.expected_bsns_decimals = self.asset.decimals;
        args.deployment_instance_id = self.asset.deployment_instance_id.clone();
        args
    }

    pub fn for_record(
        store: &crate::storage::StableStore,
        kind: crate::storage::RecordAssetKind,
        record_id: &[u8],
        require_enabled: bool,
    ) -> Result<Self, crate::storage::StorageError> {
        let asset_id = store.record_asset(kind, record_id)?;
        let root = store
            .config()?
            .ok_or(crate::storage::StorageError::RecordNotFound)?;
        let asset = store
            .asset(&asset_id)?
            .ok_or(crate::storage::StorageError::RecordNotFound)?;
        Self::new(root, asset, require_enabled)
            .map_err(|_| crate::storage::StorageError::DecodeFailed)
    }

    pub fn validates_shared_observation(
        &self,
        observation: &crate::evm_rpc::SharedAssetObservation,
    ) -> bool {
        let snapshot = observation.snapshot;
        self.is_shared()
            && snapshot.token == self.token_contract.unwrap_or([0; 20])
            && snapshot.bridge_signer.as_slice() == self.asset.expected_bridge_signer
            && observation.bridge_runtime_sha256.as_slice()
                == self.asset.expected_bridge_runtime_sha256
            && observation.token_runtime_sha256.as_slice()
                == self.asset.expected_token_runtime_sha256
            && observation.token_bridge == self.bridge_contract
            && observation.token_name == self.asset.name
            && observation.token_symbol == self.asset.symbol
            && observation.token_decimals == self.asset.decimals
    }

    pub fn attestation_from_shared_observation(
        &self,
        observation: &crate::evm_rpc::SharedAssetObservation,
    ) -> crate::config::AssetRuntimeAttestation {
        let snapshot = observation.snapshot;
        crate::config::AssetRuntimeAttestation {
            asset_id: self.asset_id.to_vec(),
            chain_id: self.asset.base_chain_id,
            finalized_block_number: observation.finalized.block_number,
            finalized_block_hash: observation.finalized.block_hash.to_vec(),
            observed_at_ns: observation.finalized.observed_at_ns,
            bridge_runtime_sha256: observation.bridge_runtime_sha256.to_vec(),
            token_runtime_sha256: observation.token_runtime_sha256.to_vec(),
            bridge_signer: snapshot.bridge_signer.to_vec(),
            token_contract: snapshot.token.to_vec(),
            token_bridge: observation.token_bridge.to_vec(),
            token_name: observation.token_name.clone(),
            token_symbol: observation.token_symbol.clone(),
            token_decimals: observation.token_decimals,
            global_epoch: snapshot.global_epoch,
            asset_epoch: snapshot.asset_epoch,
            service_fee: snapshot.mint.service_fee.get(),
            max_service_fee: snapshot.mint.max_service_fee.get(),
            per_deposit_limit: snapshot.mint.per_deposit_limit.get(),
            mint_window_limit: snapshot.mint.mint_window_limit.get(),
            mint_window_duration: snapshot.mint.mint_window_duration,
            global_deposits_paused: snapshot.global_deposits_paused,
            global_withdrawals_paused: snapshot.global_withdrawals_paused,
            asset_deposits_paused: snapshot.asset_deposits_paused,
            asset_withdrawals_paused: snapshot.asset_withdrawals_paused,
        }
    }
}

pub fn parse_asset_id(value: &[u8]) -> Result<[u8; 32], &'static str> {
    let id: [u8; 32] = value.try_into().map_err(|_| "asset ID must be 32 bytes")?;
    if id == [0; 32] {
        return Err("asset ID cannot be zero");
    }
    Ok(id)
}

pub fn custody_subaccount(asset_id: [u8; 32]) -> [u8; 32] {
    if asset_id == KINIC_ASSET_ID {
        return [0; 32];
    }
    let mut hasher = Sha256::new();
    hasher.update(b"IC_BASE_BRIDGE_ASSET_CUSTODY_V1\0");
    hasher.update(asset_id);
    hasher.finalize().into()
}

pub fn derive_deposit_id(
    deployment_instance_id: &[u8],
    canister_id: Principal,
    asset_id: [u8; 32],
    owner: Principal,
    owner_sequence: u64,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"IC_BASE_MULTI_ASSET_DEPOSIT_ID_V1\0");
    hasher.update(deployment_instance_id);
    hasher.update(canister_id.as_slice());
    hasher.update(asset_id);
    hasher.update(owner.as_slice());
    hasher.update(owner_sequence.to_be_bytes());
    hasher.finalize().into()
}

/// Shared replay keys bind the immutable asset registry entry; legacy keys stay unchanged.
pub fn notification_key(asset_id: [u8; 32], transaction_hash: [u8; 32]) -> [u8; 32] {
    if asset_id == KINIC_ASSET_ID {
        return transaction_hash;
    }
    let mut hasher = Sha256::new();
    hasher.update(b"IC_BASE_MULTI_ASSET_NOTIFICATION_V1\0");
    hasher.update(asset_id);
    hasher.update(transaction_hash);
    hasher.finalize().into()
}

/// Failed verification must not let one caller suppress another caller's proof.
pub fn notification_failure_key(
    asset_id: [u8; 32],
    transaction_hash: [u8; 32],
    caller: Principal,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"IC_BASE_NOTIFICATION_FAILURE_V1\0");
    hasher.update(asset_id);
    hasher.update(transaction_hash);
    hasher.update(caller.as_slice());
    hasher.finalize().into()
}

pub fn internal_withdrawal_id(
    asset_id: [u8; 32],
    bridge_contract: [u8; 20],
    base_withdrawal_id: [u8; 32],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"IC_BASE_MULTI_ASSET_WITHDRAWAL_ID_V1\0");
    hasher.update(asset_id);
    hasher.update(bridge_contract);
    hasher.update(base_withdrawal_id);
    hasher.finalize().into()
}

pub fn shared_domain(chain_id: u64, verifying_contract: [u8; 20]) -> MintAuthorizationDomain {
    MintAuthorizationDomain {
        name: SHARED_DOMAIN_NAME.into(),
        version: SHARED_DOMAIN_VERSION.into(),
        chain_id,
        verifying_contract,
    }
}

pub fn shared_authorization_digest(
    domain: &MintAuthorizationDomain,
    authorization: SharedMintAuthorization,
) -> [u8; 32] {
    let domain_separator = keccak(&concat_words(&[
        keccak(DOMAIN_TYPE),
        keccak(domain.name.as_bytes()),
        keccak(domain.version.as_bytes()),
        uint_word(u128::from(domain.chain_id)),
        address_word(domain.verifying_contract),
    ]));
    let value = authorization.authorization;
    let struct_hash = keccak(&concat_words(&[
        keccak(AUTHORIZATION_TYPE),
        authorization.asset_id,
        value.deposit_id,
        address_word(value.recipient),
        uint_word(value.gross_amount.get()),
        uint_word(value.max_service_fee.get()),
        uint_word(value.charged_service_fee.get()),
        uint_word(u128::from(value.deadline)),
        uint_word(u128::from(authorization.global_epoch)),
        uint_word(u128::from(authorization.asset_epoch)),
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
    let mut output = [0; 32];
    let mut hasher = Keccak::v256();
    hasher.update(bytes);
    hasher.finalize(&mut output);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_failure_keys_isolate_callers_and_assets() {
        let caller = Principal::from_slice(&[1]);
        let other = Principal::from_slice(&[2]);
        let tx = [9; 32];
        let key = notification_failure_key(KINIC_ASSET_ID, tx, caller);
        assert_ne!(key, notification_failure_key(KINIC_ASSET_ID, tx, other));
        assert_ne!(key, notification_failure_key([8; 32], tx, caller));
        assert_ne!(
            notification_key(KINIC_ASSET_ID, tx),
            notification_key([8; 32], tx)
        );
        assert_eq!(key, notification_failure_key(KINIC_ASSET_ID, tx, caller));
    }

    #[test]
    fn additional_assets_use_distinct_nonzero_custody_subaccounts() {
        let first = custody_subaccount([0x11; 32]);
        let second = custody_subaccount([0x12; 32]);
        assert_ne!(first, [0; 32]);
        assert_ne!(first, second);
        assert_eq!(custody_subaccount(KINIC_ASSET_ID), [0; 32]);
    }

    #[test]
    fn ids_and_authorizations_are_asset_bound() {
        let owner = Principal::from_slice(&[0x99]);
        let first = derive_deposit_id(&[1; 32], Principal::from_slice(&[1]), [2; 32], owner, 7);
        let second = derive_deposit_id(&[1; 32], Principal::from_slice(&[1]), [3; 32], owner, 7);
        assert_ne!(first, second);

        let value = MintAuthorization {
            deposit_id: first,
            recipient: [4; 20],
            gross_amount: Amount::new(100),
            max_service_fee: Amount::new(10),
            charged_service_fee: Amount::new(5),
            deadline: 1_000,
            authorization_epoch: 9,
        };
        let domain = shared_domain(8453, [5; 20]);
        let digest = shared_authorization_digest(
            &domain,
            SharedMintAuthorization {
                asset_id: [2; 32],
                authorization: value,
                global_epoch: 9,
                asset_epoch: 3,
            },
        );
        assert_ne!(
            digest,
            shared_authorization_digest(
                &domain,
                SharedMintAuthorization {
                    asset_id: [3; 32],
                    authorization: value,
                    global_epoch: 9,
                    asset_epoch: 3,
                },
            )
        );
    }
}
