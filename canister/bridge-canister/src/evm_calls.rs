use crate::config::BridgeInitArgs;
use bridge_core::{EvmCallIntent, EvmFeeQuote, EvmOperationId};
use tiny_keccak::{Hasher, Keccak};

fn selector(signature: &str) -> [u8; 4] {
    let mut hash = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(signature.as_bytes());
    hasher.finalize(&mut hash);
    hash[..4].try_into().expect("four-byte selector")
}

fn word(value: u128) -> [u8; 32] {
    let mut encoded = [0u8; 32];
    encoded[16..].copy_from_slice(&value.to_be_bytes());
    encoded
}

fn intent(
    config: &BridgeInitArgs,
    operation_id: EvmOperationId,
    payload_hash: [u8; 32],
    calldata: Vec<u8>,
    fee_quote: EvmFeeQuote,
) -> EvmCallIntent {
    EvmCallIntent {
        operation_id,
        payload_hash,
        chain_id: config.base_chain_id,
        contract: config.contract_array(),
        calldata,
        gas_limit: fee_quote.gas_limit,
        max_fee_per_gas: fee_quote.initial_max_fee_per_gas,
        max_priority_fee_per_gas: fee_quote.max_priority_fee_per_gas,
        fee_quote: Some(fee_quote),
    }
}

pub struct MintDepositArgs {
    pub deposit_id: [u8; 32],
    pub recipient: [u8; 20],
    pub gross_amount: u128,
    pub max_service_fee: u128,
    pub charged_service_fee: u128,
}

pub fn mint_deposit(
    config: &BridgeInitArgs,
    operation_id: EvmOperationId,
    payload_hash: [u8; 32],
    args: MintDepositArgs,
    fee_quote: EvmFeeQuote,
) -> EvmCallIntent {
    let calldata = mint_deposit_calldata(&args);
    intent(config, operation_id, payload_hash, calldata, fee_quote)
}

pub fn mint_deposit_calldata(args: &MintDepositArgs) -> Vec<u8> {
    let mut calldata = selector("mintDeposit((bytes32,address,uint256,uint256,uint256))").to_vec();
    calldata.extend_from_slice(&args.deposit_id);
    calldata.extend_from_slice(&[0; 12]);
    calldata.extend_from_slice(&args.recipient);
    calldata.extend_from_slice(&word(args.gross_amount));
    calldata.extend_from_slice(&word(args.max_service_fee));
    calldata.extend_from_slice(&word(args.charged_service_fee));
    calldata
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid::Principal;

    fn config() -> BridgeInitArgs {
        BridgeInitArgs {
            ledger_canister_id: Principal::anonymous(),
            index_canister_id: Principal::anonymous(),
            evm_rpc_canister_id: Principal::anonymous(),
            custom_evm_rpc_urls: vec![],
            base_chain_id: 8453,
            bridge_contract: vec![0x77; 20],
            timelock_contract: vec![0x78; 20],
            ecdsa_key_name: "test".into(),
            ecdsa_derivation_path: vec![],
            governance_ecdsa_derivation_path: vec![],
            deposit_rate_limit_window_seconds: 1,
            deposit_rate_limit_global: 1,
            deposit_rate_limit_per_principal: 1,
            settlement_rate_limit_window_seconds: 1,
            settlement_rate_limit_global: 1,
            settlement_rate_limit_per_principal: 1,
            settlement_rate_limit_per_record: 1,
            evm_fee: crate::config::EvmFeePolicy {
                gas_limit_ceiling: 500_000,
                max_fee_per_gas_ceiling: 10,
                max_priority_fee_per_gas_ceiling: 1,
                l1_fee_per_transaction_ceiling_wei: 1,
                quote_validity_seconds: 90,
                gas_limit_multiplier_bps: 13_000,
                base_fee_multiplier_bps: 60_000,
                l1_fee_multiplier_bps: 15_000,
            },
            evm_liveness: crate::config::EvmLivenessPolicy::default(),
            eth_floor_wei: 0,
            cycles_floor: 0,
            settlement_cycle_ceiling: 0,
            governance_principal: Principal::anonymous(),
            pause_principal: Principal::anonymous(),
            fee_recipient: crate::config::FeeRecipientConfig {
                owner: Principal::anonymous(),
                subaccount: vec![],
            },
        }
    }

    fn quote() -> EvmFeeQuote {
        EvmFeeQuote {
            safe_block_number: 1,
            safe_block_hash: [1; 32],
            observed_at_ns: 1,
            valid_until_ns: 2,
            base_fee_per_gas: 1,
            max_priority_fee_per_gas: 1,
            gas_estimate: 400_000,
            gas_limit: 500_000,
            initial_max_fee_per_gas: 10,
            reachable_max_fee_per_gas: 10,
            observed_l1_fee_upper_bound_wei: 1,
            reserved_l1_fee_wei: 1,
            reserved_eth_wei: 5_000_001,
        }
    }

    fn assert_binding(intent: &EvmCallIntent) {
        assert_eq!(intent.operation_id, EvmOperationId::new(9));
        assert_eq!(intent.payload_hash, [0x55; 32]);
        assert_eq!(intent.chain_id, 8453);
        assert_eq!(intent.contract, [0x77; 20]);
        assert_eq!(intent.gas_limit, 500_000);
        assert_eq!(intent.max_fee_per_gas, 10);
        assert_eq!(intent.max_priority_fee_per_gas, 1);
    }

    #[test]
    fn typed_calls_bind_selectors_arguments_and_envelope() {
        let config = config();
        let operation_id = EvmOperationId::new(9);
        let payload_hash = [0x55; 32];
        let id = [0x11; 32];
        let mint = mint_deposit(
            &config,
            operation_id,
            payload_hash,
            MintDepositArgs {
                deposit_id: id,
                recipient: [0x22; 20],
                gross_amount: 1,
                max_service_fee: 2,
                charged_service_fee: 3,
            },
            quote(),
        );
        assert_eq!(mint.calldata.len(), 4 + 32 * 5);
        assert_eq!(&mint.calldata[..4], &[0x84, 0xc7, 0x27, 0xfe]);
        assert_eq!(&mint.calldata[4..36], &id);
        assert_eq!(&mint.calldata[36..48], &[0; 12]);
        assert_eq!(&mint.calldata[48..68], &[0x22; 20]);
        assert_eq!(&mint.calldata[68..100], &word(1));
        assert_eq!(&mint.calldata[100..132], &word(2));
        assert_eq!(&mint.calldata[132..164], &word(3));
        assert_binding(&mint);
    }
}
