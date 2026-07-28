---
status: superseded
superseded_by: ADR 0023
normative: false
---

# Deposit EVM confirmationをフロント通知で開始する

> **非規範的な履歴文書:** [ADR 0023](../../0023-use-wallet-funded-eip712-mint-authorization.md)が本ADRをsupersedeした。以下は旧Canister発Mint transaction方式の履歴であり、実装根拠として使用しない。

Canister発のEVM operationは`MintDeposit`だけである。フロントはreceiptとFinalized headを観測し、認証済みIC walletから`confirm_deposit`を呼ぶ。Canisterは保存済みtransactionとの一致とcanonical Finalized到達をEVM RPC quorumで再検証する。

WithdrawalにはCanister発EVM transactionがないため、`confirm_withdrawal`、pending EVM confirmation、timer fallbackを持たない。`continue_withdrawal`はLedger transferまたは履歴照合だけを進める。
