---
status: superseded
---

# Settlement confirmationの自動確認

ADR 0020がsupersedeする。WithdrawalのBase acknowledgement自体がADR 0018で廃止されたため、Withdrawal用confirmation scheduleも存在しない。Depositの`MintDeposit` confirmationだけをフロント通知で開始する。
