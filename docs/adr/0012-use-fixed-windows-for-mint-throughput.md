---
status: accepted
---

# Mint Throughput Limitをfixed windowで実装する

Base contractのMint Throughput Limitは、sliding windowではなく一定間隔でリセットされるfixed window（初期値1時間）で実装する。contractは非アップグレード型であり、windowごとの累積量1つで済むfixed windowは状態とSMTCheckerの証明対象を最小にできる。

## Considered Options

- sliding windowはwindow境界をまたぐバーストを許さないが、mint履歴の保持または近似構造をimmutable contractへ持ち込むため不採用とする。
- fixed windowを採用し、境界バーストは上限値の設定で織り込む。

## Consequences

- window境界をまたぐ短時間に、最大でwindow上限の2倍がmintされうる。Mint Throughput Limitの値は許容最大被害額をこの2倍で割って導出する（`docs/parameters.md`）。
- windowの長さと上限値の変更はADR 0009の危険方向の操作（引き上げ）と安全方向の操作（引き下げ）に従う。
