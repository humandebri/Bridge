//! SNS proposal boundary for additional assets; successful reply means the IC intent was accepted.
use crate::{admin, base_governance, config, multi_asset, ActionKey, InFlightGuard, STORE};
use candid::{CandidType, Deserialize, Principal};

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SnsAssetProposal {
    pub bridge_canister_id: Principal,
    pub deployment_instance_id: Vec<u8>,
    pub operational_config_sha256: Vec<u8>,
    pub expires_at_ns: u64,
    pub action: SnsAssetAction,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum SnsAssetAction {
    RegisterAsset(config::AssetConfig),
    RefreshRuntime {
        asset_id: Vec<u8>,
        expected_observed_at_ns: Option<u64>,
    },
    PrepareBase {
        expected_operation_id: u64,
        action: base_governance::BaseGovernanceAction,
    },
}

pub(crate) fn validate(proposal: &SnsAssetProposal) -> Result<String, String> {
    let runtime = crate::get_runtime_binding();
    let paused = STORE
        .with(|store| store.borrow().admin_state())
        .map_err(|e| format!("{e:?}"))?
        .deposits_paused;
    let activated = base_governance::production_lifecycle().map_err(|e| format!("{e:?}"))?
        == base_governance::ProductionLifecycle::Activated;
    if !bridge_core::asset_governance_policy::sns_asset_proposal_allowed(
        proposal.bridge_canister_id == ic_cdk::api::canister_self()
            && proposal.deployment_instance_id == runtime.deployment_instance_id
            && proposal.operational_config_sha256 == runtime.operational_config_sha256
            && runtime.schema_version == 37
            && runtime.kinic_asset_binding_valid,
        activated,
        !paused,
        ic_cdk::api::time(),
        proposal.expires_at_ns,
        true,
    ) {
        return Err(
            "Proposal domain, lifecycle or admission deadline differs from current v37 state"
                .into(),
        );
    }
    match &proposal.action {
        SnsAssetAction::RegisterAsset(asset) => {
            crate::validate_new_asset(asset).map_err(|e| format!("{e:?}"))?
        }
        SnsAssetAction::RefreshRuntime {
            asset_id,
            expected_observed_at_ns,
        } => {
            let id = multi_asset::parse_asset_id(asset_id).map_err(str::to_string)?;
            STORE.with(|store| {
                let store = store.borrow();
                let asset = store
                    .asset(&id)
                    .map_err(|e| format!("{e:?}"))?
                    .ok_or("Unknown asset")?;
                if asset.bridge_kind != config::BaseBridgeKind::SharedMultiToken {
                    return Err("Legacy KINIC uses its existing runtime path".into());
                }
                let observed = store
                    .asset_runtime_attestation(asset_id)
                    .map_err(|e| format!("{e:?}"))?
                    .map(|a| a.observed_at_ns);
                if &observed != expected_observed_at_ns {
                    return Err("Runtime refresh predecessor changed".into());
                }
                Ok::<(), String>(())
            })?;
        }
        SnsAssetAction::PrepareBase {
            expected_operation_id,
            action,
        } => base_governance::validate_sns_asset_action(action, *expected_operation_id)?,
    }
    Ok(format!("Manage additional IC–Base asset on {}. Instance {:?}; expires {} ns; action {:?}. Base transactions require separate relay and Finalized confirmation.",proposal.bridge_canister_id,proposal.deployment_instance_id,proposal.expires_at_ns,proposal.action))
}

pub(crate) async fn execute(proposal: SnsAssetProposal) -> Result<(), String> {
    let caller = ic_cdk::api::msg_caller();
    if !admin::is_governance(caller).unwrap_or(false) {
        return Err("Only SNS Governance may execute this proposal".into());
    }
    validate(&proposal)?;
    match &proposal.action {
        SnsAssetAction::RegisterAsset(asset) => {
            crate::register_asset(asset.clone()).map_err(|e| format!("{e:?}"))?
        }
        SnsAssetAction::RefreshRuntime { asset_id, .. } => {
            crate::refresh_asset_runtime_inner(asset_id.clone(), Some(&proposal))
                .await
                .map_err(|e| format!("{e:?}"))?;
        }
        SnsAssetAction::PrepareBase { action, .. } => {
            let _guard = InFlightGuard::acquire(ActionKey::BaseGovernance)
                .ok_or("Base governance operation is in flight")?;
            base_governance::prepare_sns_asset(caller, action.clone().into(), &proposal)
                .await
                .map_err(|e| format!("{e:?}"))?;
        }
    }
    Ok(())
}
