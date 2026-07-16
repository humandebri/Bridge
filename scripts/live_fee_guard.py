#!/usr/bin/env python3
import json
import re
import sys


def candid_nat(value):
    if isinstance(value, bool):
        raise ValueError("Ledger fee is not a nat")
    if isinstance(value, int):
        if value < 0:
            raise ValueError("Ledger fee is not a nat")
        return value
    if isinstance(value, str):
        digits = re.sub(r"[_\s]", "", value.strip().strip('"'))
        if digits.isdigit():
            return int(digits)
        raise ValueError("Ledger fee is not a nat")
    if isinstance(value, list) and len(value) == 1:
        return candid_nat(value[0])
    if isinstance(value, dict) and len(value) == 1:
        return candid_nat(next(iter(value.values())))
    raise ValueError("Ledger fee response has an unexpected shape")


def validate(profile, base_state, ledger_reply):
    expected_ledger = int(profile["parameters"]["ledger_fee"])
    expected_service = int(profile["parameters"]["service_fee"])
    ledger_fee = candid_nat(ledger_reply)
    base_service_fee = int(base_state["state"]["base_service_fee"])
    if ledger_fee != expected_ledger:
        raise ValueError("Ledger fee drift")
    if base_service_fee != expected_service:
        raise ValueError("Base service fee drift")
    if ledger_fee > base_service_fee:
        raise ValueError("Ledger fee exceeds Base service fee")
    return {"base_service_fee": base_service_fee, "ledger_fee": ledger_fee}


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: live_fee_guard.py PROFILE BASE_STATE LEDGER_FEE_RESPONSE")
    with open(sys.argv[1], encoding="utf-8") as source:
        profile = json.load(source)
    with open(sys.argv[2], encoding="utf-8") as source:
        base_state = json.load(source)
    with open(sys.argv[3], encoding="utf-8") as source:
        ledger_reply = json.load(source)
    try:
        result = validate(profile, base_state, ledger_reply)
    except (KeyError, TypeError, ValueError) as error:
        raise SystemExit(str(error)) from error
    print(json.dumps(result, sort_keys=True, separators=(",", ":")))


if __name__ == "__main__":
    main()
