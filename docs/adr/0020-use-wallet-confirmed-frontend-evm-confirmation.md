---
status: accepted
---

# Deposit EVM confirmationをフロント通知で開始する

Canister発のEVM operationは`MintDeposit`だけである。フロントはreceiptとFinalized headを観測し、認証済みIC walletから`confirm_deposit`を呼ぶ。Canisterは保存済みtransactionとの一致とcanonical Finalized到達をEVM RPC quorumで再検証する。

WithdrawalにはCanister発EVM transactionがないため、`confirm_withdrawal`、pending EVM confirmation、timer fallbackを持たない。`continue_withdrawal`はLedger transferまたは履歴照合だけを進める。
