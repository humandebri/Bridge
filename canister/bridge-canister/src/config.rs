use candid::Principal;

pub const KINIC_LEDGER_CANISTER_ID: &str = "73mez-iiaaa-aaaaq-aaasq-cai";
pub const KINIC_INDEX_CANISTER_ID: &str = "7vojr-tyaaa-aaaaq-aaatq-cai";
pub const KINIC_DECIMALS: u8 = 8;

pub fn kinic_ledger_canister_id() -> Principal {
    Principal::from_text(KINIC_LEDGER_CANISTER_ID)
        .expect("the fixed KINIC ledger canister ID must be valid")
}

pub fn kinic_index_canister_id() -> Principal {
    Principal::from_text(KINIC_INDEX_CANISTER_ID)
        .expect("the fixed KINIC index canister ID must be valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_kinic_canister_ids_are_valid_and_distinct() {
        assert_eq!(
            kinic_ledger_canister_id().to_text(),
            KINIC_LEDGER_CANISTER_ID
        );
        assert_eq!(kinic_index_canister_id().to_text(), KINIC_INDEX_CANISTER_ID);
        assert_ne!(kinic_ledger_canister_id(), kinic_index_canister_id());
    }
}
