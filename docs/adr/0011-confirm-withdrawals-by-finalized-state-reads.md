---
status: accepted
---

# Withdrawalをfinalizedの状態読みで確定する

Bridge canisterはBaseのWithdrawalを、`finalized` block tagに対するcontract状態の読み取りで確定する。イベントログは新規Withdrawalの発見にだけ使い、mintとICP Releaseの判断根拠にしない。読み取りはEVM RPC canisterの複数provider合意（3中2）を要求する。

## Considered Options

- `eth_getLogs`の結果を直接判断根拠にする案は、reorgとprovider間の応答差異の影響を受けやすく、withdrawal IDをキーとする冪等照合とも噛み合わないため不採用とする。
- `latest`または`safe`を基準にする案は、mintを誘発する読み取りでreorgを踏むことが二重発行と同義であるため不採用とする。
- `finalized`タグでの状態読みを採用する。

## Consequences

- Withdrawal確定までの遅延としてL1 finalize（実用上10〜20分程度）を受け入れる。
- pollingはtimerで1〜5分間隔とし、finalityの粒度より細かくしない。
- 状態読みの対象はwithdrawal IDをキーとする記録とし、ADR 0003の冪等なRelease acknowledgementと対応させる。
- 2/3のRPC providerが同時に誤る可能性への残余信頼は、ADR 0005の外部仮定監査リストへ載せる。
