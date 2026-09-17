# Concrete ABI snapshots

`Bridge.json` and `BSNS.json` are canonical snapshots of the concrete contract ABIs frozen in Phase 1E.

Regenerate snapshots explicitly as follows; normal validation uses `--check` only to reject differences.

```bash
python3 scripts/abi_snapshot.py --update
python3 scripts/abi_snapshot.py --check
```

Foundry fixtures also check interface subsets, constructor argument order, struct field order, enum ordinals, and function/error/event selectors and topics.
