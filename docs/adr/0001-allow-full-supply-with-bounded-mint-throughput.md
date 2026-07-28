---
status: accepted
---

# 全量移動を許容し、Deposit単位とmint流量を制限する

BridgeはSNSトークン全量がBaseへ移動する可能性を受け入れるため、Bridge Exposureに総量上限を設けない。安全制御は、非アップグレード型Base contractが強制するPer-Deposit LimitとMint Throughput Limitで行う。1回上限だけでは要求分割で回避できるため、短時間のmint総量も制限する。

## Considered Options

- Bridge ExposureをSNS総供給の一定割合に制限する案は、全量移動という目的を妨げるため不採用とする。
- Per-Deposit Limitだけを設ける案は、連続要求で被害上限にならないため不採用とする。
- Per-Deposit LimitとMint Throughput Limitを併用し、時間経過により全量移動を可能にする案を採用する。

## Consequences

- 制限対象は新規Deposit mintだけとする。WithdrawalはBase上でburnした時点で`Committed`となり、refund mintや再mint経路を持たない。
- 各DepositへPer-Deposit Limitを適用し、同じfixed window内の新規mint量を共有Mint Throughput Limitへ累積する。
- 制限値はraw unitで定義し、token decimalsの表示変換を安全判断へ使用しない。
- Verusで、各Depositが1回上限を超えないことと、mint流量が予約済み量を含めて保存されることを証明対象にする。
- 本決定は累積移動量またはBridge Exposureの上限を保証しない。短時間の障害・侵害・入力ミスに対する被害速度を制御する。
