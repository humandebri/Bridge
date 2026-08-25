# 条件付きliveness補助定理

`conditional-liveness.tsv`の5件はrelease-safety claimではなく、Leanの補助定理である。
いずれも、対象の終端遷移が選択されるまでadmissibleであり続けること、weak fairness、
外部system・storage・time・cyclesの進行、および列挙されたuserまたはkeeper actionを
前件に置く。

proof gateのpassが示すのは、これらの含意がproject-local axiomなしにtypecheckすること
だけである。production schedulerや外部systemが前件を満たすこと、またはproduction
実装からadmissibilityが導出できることは証明していない。このためrelease summaryでは
`conditional-liveness`として分離し、`release-ready`にも`implementation-proved`にも
数えない。
