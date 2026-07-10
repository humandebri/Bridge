---
status: accepted
---

# bSNSにSNS Governanceの権利を持たせない

bSNSはBridgeable SNS Tokenを1:1で裏付けるBase上の流通トークンに限定し、SNS Governanceの投票権、neuron権限、投票報酬を持たせない。Governanceへ参加する利用者はbSNSをburnしてICP上のSNSトークンをreleaseし、neuronへstakeする。

## Considered Options

- Base保有量をsnapshotしてcross-chain votingを提供する案は、transfer中の投票権、二重投票防止、delegation、投票期間中の残高固定が新たなprotocolを必要とするため不採用とする。
- Bridge canisterがescrow残高を代理stake・代理投票する案は、利用者ごとの意思を表現できず、Bridgeへ過大なGovernance権限を集中させるため不採用とする。
- bSNSを投票権のない1:1裏付けtokenとする案を採用する。

## Consequences

- neuronへstake中のSNSトークンはBridgeable SNS Tokenではない。利用者はneuronをdissolveしてdisburseするまでDepositできない。
- Bridge escrowのSNSトークンをstake、投票、treasury運用、貸付へ使用しない。
- UIはDeposit前に、bSNSでは投票と投票報酬を得られないことを明示する。
- bSNS所有者へSNS neuron permissionを付与せず、Base addressとPrincipalのGovernance用identity mappingを導入しない。
- Bridgeの会計と形式証明は資産の1:1裏付けに限定し、cross-chain governanceを証明範囲へ含めない。
