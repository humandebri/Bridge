---
status: accepted
---

# Bridgeを単一SNSトークン専用にデプロイする

Bridge canisterとBase contractの1組は単一のSNSトークン専用とし、複数SNSを1つのcanisterで扱わない。ADR 0008のhandoverはcontrollerが当該SNS Rootだけであることを完了条件とするため、複数SNSを共有するcanisterはどのSNS Rootへも移管できず、handover設計と両立しない。

## Considered Options

- 複数SNSを単一canisterで扱うmulti-tenant案は、upgrade権限を単一のSNS Governanceへ帰属できず、pause、Settlement Reserve、cycles残高がSNS間で干渉するため不採用とする。
- SNSごとに独立したcanisterとcontractの組をデプロイする案を採用する。

## Consequences

- stateとデプロイ構成からtoken IDによる分岐を排除する。
- Settlement Reserve、fee reserve、Reconciliation Holdの会計は1トークンに閉じる。
- コードはSNSを問わず再利用できる形で書くが、factory化は必要になるまで導入しない。
- 複数SNSへ展開する場合、組ごとに別個のhandover、Verus証明、監査を実施する。
