# Concrete ABI snapshots

`Bridge.json`と`BSNS.json`はPhase 1Eで凍結するconcrete contract ABIのcanonical snapshotである。

snapshotは次で明示的に再生成し、通常の検証は`--check`で差分だけを拒否する。

```bash
python3 scripts/abi_snapshot.py --update
python3 scripts/abi_snapshot.py --check
```

interfaceのsubset、constructor引数順、struct field順、enum ordinal、function・error・eventのselectorとtopicはFoundry fixtureでも検査する。
