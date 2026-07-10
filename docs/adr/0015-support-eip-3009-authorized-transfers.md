---
status: accepted
---

# bSNSでEIP-3009署名送金を提供する

bSNSはERC-20を維持し、Base上のx402 `exact`決済で直接利用できるようにEIP-3009の署名送金を実装する。
標準ERC-20のままPermit2だけを使う方法でもx402決済は可能だが、外部proxyに依存せず、利用者の1回の署名をfacilitatorがsettleできる経路をbSNS自身で提供する。
bSNSは非アップグレード型であり、後からの機能追加には再deployが必要になるため、Phase 1のinterface凍結前に採用する。

## Considered Options

- 標準ERC-20とPermit2だけを使う案は、token contractの実装を増やさない一方で、x402決済を外部のPermit2 contractとproxyに依存させるため、唯一の経路にはしない。
  通常のERC-20 allowanceは維持するため、Permit2も代替経路として利用できる。
- ERC-2612だけを追加する案は、署名によってallowanceを設定する規格であり、x402のEIP-3009直接送金を提供しないため採用しない。
- ERC-721を追加する案は、非代替tokenの所有権移転を定義する規格であり、ICRC-1 tokenをraw unitで1:1に裏付けるbSNSの性質と一致しないため採用しない。

## Consequences

- bSNSは`transferWithAuthorization`、`receiveWithAuthorization`、`authorizationState`、`cancelAuthorization`を提供し、authorizationの使用と取消しをeventに記録する。
- EIP-712 domainはtoken name、固定version `"1"`、実行chain ID、bSNS contract addressへ署名を束縛する。
  別chainまたは別contractで作られた署名を受理しない。
  固定versionは`version()`、domain全体はEIP-5267 `eip712Domain()`から取得可能にする。
- authorization nonceはauthorizerごとの単一namespaceで管理する。
  使用済みまたは取消済みのnonceは、どのauthorization送金関数からも再利用できない。
- `receiveWithAuthorization`はcallerと受取人の一致を要求し、第三者が署名を流用して送金だけを先に実行するfront-runningを防ぐ。
- authorization送金は既存balanceの移転だけを許可し、mintとburnの権限を追加しない。
  Bridgeだけが供給量を操作するという制約は維持する。
- x402 resource serverとfacilitatorの運用はBridgeの責務に含めない。
  本番前に、採用時点のx402仕様とBase上のfacilitatorで互換性を再検証する。

EIP-3009の署名形式とsecurity considerationsは[EIP-3009](https://eips.ethereum.org/EIPS/eip-3009)を正本とする。
x402で利用可能なEVM tokenの判定は[x402 Network & Token Support](https://docs.x402.org/core-concepts/network-and-token-support)に従い、本番前に変更の有無を確認する。
