# ADR archive

このディレクトリ以下は、後続ADRにより置換された非規範的な意思決定履歴である。実装、運用、レビューの正本として使用しない。

## Deposit confirmation

- [ADR 0017: Settlement confirmationの自動確認](deposit-confirmation/0017-automate-settlement-confirmation.md)
- [ADR 0020: Deposit EVM confirmationをフロント通知で開始する](deposit-confirmation/0020-use-wallet-confirmed-frontend-evm-confirmation.md)

これらが前提とした`MintDeposit`、`confirm_deposit`、Canister発Mint transactionは廃止済みである。Deposit Mintの現行仕様は[ADR 0023: Base walletが送信するEIP-712 Mint Authorizationを使う](../0023-use-wallet-funded-eip712-mint-authorization.md)を正本とする。
