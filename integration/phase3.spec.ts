import { readFileSync } from "node:fs";
import { spawn, type ChildProcess } from "node:child_process";
import { createServer } from "node:net";
import { resolve } from "node:path";
import { IDL } from "@icp-sdk/core/candid";
import { Principal } from "@icp-sdk/core/principal";
import { PocketIc, SubnetStateType } from "@dfinity/pic";

const root = resolve(__dirname, "..");
const bridgeWasm = resolve(root, "target/test-deployment/wasm32-unknown-unknown/release/bridge_canister.wasm");
const mockWasm = resolve(root, "target/wasm32-unknown-unknown/release/mock_external.wasm");

const mockInit = IDL.Record({ ledger_id: IDL.Principal });
const ledgerMode = IDL.Variant({ Succeed: IDL.Null, Duplicate: IDL.Null, Trap: IDL.Null, BadFee: IDL.Null, TemporarilyUnavailable: IDL.Null });
const receiptMode = IDL.Variant({ Confirmed: IDL.Null, Missing: IDL.Null, Reverted: IDL.Null, RpcFailure: IDL.Null, Inconsistent: IDL.Null, DecodeFailure: IDL.Null, Orphaned: IDL.Null });
const chainIdMode = IDL.Variant({ Configured: IDL.Null, Wrong: IDL.Null, Inconsistent: IDL.Null });
const blockMode = IDL.Variant({ Canonical: IDL.Null, SafeInconsistent: IDL.Null, CanonicalInconsistent: IDL.Null, SameHeightDifferentHash: IDL.Null });
const withdrawalFixture = IDL.Record({ id: IDL.Vec(IDL.Nat8), owner: IDL.Vec(IDL.Nat8), subaccount: IDL.Vec(IDL.Nat8), amount: IDL.Nat, min_amount_out: IDL.Nat });
const chainKeyProbe = IDL.Record({ public_key: IDL.Vec(IDL.Nat8), signature: IDL.Vec(IDL.Nat8) });
const mockIdl = ({ IDL: I }: { IDL: typeof IDL }) =>
  I.Service({
    set_ledger_mode: I.Func([ledgerMode], [], []),
    set_ledger_fee: I.Func([I.Nat], [], []),
    set_bad_fee_expected_fee: I.Func([I.Opt(I.Nat)], [], []),
    set_ledger_fee_available: I.Func([I.Bool], [], []),
    set_withdrawal: I.Func([I.Opt(withdrawalFixture)], [], []),
    set_withdrawal_status: I.Func([I.Nat8], [], []),
    set_receipt_mode: I.Func([receiptMode], [], []),
    set_chain_id_mode: I.Func([chainIdMode], [], []),
    set_block_mode: I.Func([blockMode], [], []),
    set_observed_transaction: I.Func([I.Vec(I.Nat8), I.Vec(I.Nat8), I.Vec(I.Nat8), I.Nat64], [I.Variant({ Ok: I.Null, Err: I.Text })], []),
    set_eth_balance: I.Func([I.Nat], [], []),
    set_next_evm_nonce: I.Func([I.Nat64], [], []),
    set_service_fee: I.Func([I.Nat], [], []),
    set_mint_window: I.Func([I.Nat, I.Nat, I.Nat64, I.Nat64, I.Nat64], [], []),
    set_safe_block_sequence: I.Func([I.Vec(I.Nat64)], [], []),
    set_bridge_signer_for_canister: I.Func([I.Principal, I.Text], [I.Variant({ Ok: I.Null, Err: I.Text })], []),
    set_bridge_signer: I.Func([I.Vec(I.Nat8)], [I.Variant({ Ok: I.Null, Err: I.Text })], []),
    bridge_signer: I.Func([], [I.Vec(I.Nat8)], ["query"]),
    set_deposit_mints_paused: I.Func([I.Bool], [], []),
    broadcast_transactions: I.Func([], [I.Vec(I.Vec(I.Nat8))], ["query"]),
    ledger_transactions: I.Func([], [I.Vec(I.Record({ kind: I.Text, mint: I.Opt(I.Reserved), burn: I.Opt(I.Reserved), transfer: I.Opt(I.Reserved), approve: I.Opt(I.Reserved), fee_collector: I.Opt(I.Reserved), timestamp: I.Nat64 }))], ["query"]),
    ledger_transfer_calls: I.Func([], [I.Nat64], ["query"]),
    eth_call_count: I.Func([], [I.Nat64], ["query"]),
    pinned_eth_call_block_numbers: I.Func([], [I.Vec(I.Nat64)], ["query"]),
    receipt_call_count: I.Func([], [I.Nat64], ["query"]),
    probe_chain_key: I.Func([I.Text], [I.Variant({ Ok: chainKeyProbe, Err: I.Text })], []),
  });

const bridgeInit = IDL.Record({
  ledger_canister_id: IDL.Principal,
  index_canister_id: IDL.Principal,
  evm_rpc_canister_id: IDL.Principal,
  custom_evm_rpc_urls: IDL.Vec(IDL.Text),
  base_chain_id: IDL.Nat64,
  bridge_contract: IDL.Vec(IDL.Nat8),
  ecdsa_key_name: IDL.Text,
  ecdsa_derivation_path: IDL.Vec(IDL.Vec(IDL.Nat8)),
  deposit_rate_limit_window_seconds: IDL.Nat64,
  deposit_rate_limit_global: IDL.Nat16,
  deposit_rate_limit_per_principal: IDL.Nat16,
  settlement_rate_limit_window_seconds: IDL.Nat64,
  settlement_rate_limit_global: IDL.Nat16,
  settlement_rate_limit_per_principal: IDL.Nat16,
  settlement_rate_limit_per_record: IDL.Nat16,
  transaction_gas_limit: IDL.Nat,
  max_fee_per_gas: IDL.Nat,
  max_priority_fee_per_gas: IDL.Nat,
  eth_floor_wei: IDL.Nat,
  cycles_floor: IDL.Nat,
  settlement_cycle_ceiling: IDL.Nat,
  governance_principal: IDL.Principal,
  pause_principals: IDL.Vec(IDL.Principal),
  finance_administrator: IDL.Principal,
  fee_recipient: IDL.Record({ owner: IDL.Principal, subaccount: IDL.Vec(IDL.Nat8) }),
});
const depositArgs = IDL.Record({
  owner_sequence: IDL.Nat64,
  base_recipient: IDL.Vec(IDL.Nat8),
  from_subaccount: IDL.Opt(IDL.Vec(IDL.Nat8)),
  gross_amount: IDL.Nat,
  max_service_fee: IDL.Nat,
});
const settlementStopReason = IDL.Variant({ LedgerRejected: IDL.Text, RpcUnavailable: IDL.Null, TransactionNotConfirmed: IDL.Null, ConfirmationCheckExhausted: IDL.Null, RpcInconsistent: IDL.Null, LedgerAmbiguous: IDL.Null, LedgerUnavailable: IDL.Null, LedgerFeeChanged: IDL.Null, NonceUnavailable: IDL.Null, NonceConflict: IDL.Null, TransactionReverted: IDL.Null, NonceBlocked: IDL.Null, TransactionNotFound: IDL.Null, SigningUnavailable: IDL.Null, InvalidBaseResponse: IDL.Null, BaseStateMismatch: IDL.Null, BridgeSignerMismatch: IDL.Null });
const settlementAction = IDL.Variant({ Stopped: IDL.Record({ state: IDL.Text, reason: settlementStopReason }), Complete: IDL.Record({ state: IDL.Text }), ReconciliationProgress: IDL.Record({ state: IDL.Text }), WaitingForConfirmation: IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8), state: IDL.Text }), Submitted: IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8), state: IDL.Text }) });
const settlementActionError = IDL.Variant({ AutomaticProgressPending: IDL.Record({ next_run_at_ns: IDL.Opt(IDL.Nat64) }), RateLimited: IDL.Record({ retry_after_seconds: IDL.Nat64 }), InvalidId: IDL.Null, Busy: IDL.Null, NotFound: IDL.Null, Unauthorized: IDL.Null, StorageFailure: IDL.Null, AnonymousCaller: IDL.Null });
const depositReceipt = IDL.Record({ deposit_id: IDL.Vec(IDL.Nat8), owner_sequence: IDL.Nat64, state: IDL.Text, settlement: IDL.Opt(settlementAction) });
const depositError = IDL.Variant({
  Busy: IDL.Null,
  BaseObservationUnavailable: IDL.Null,
  Rejected: IDL.Text,
  InvalidRequest: IDL.Text,
  LedgerFeeUnavailable: IDL.Null,
  StorageFailure: IDL.Null,
  DepositsPaused: IDL.Null,
  ReserveUnavailable: IDL.Null,
  RateLimited: IDL.Record({ retry_after_seconds: IDL.Nat64 }),
  SequenceMismatch: IDL.Record({ expected: IDL.Nat64 }),
  DepositConflict: IDL.Null,
});
const baseConfirmation = IDL.Variant({
  Submitted: IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8) }),
  Confirmed: IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8), receipt_block_number: IDL.Nat64, confirmed_head_block_number: IDL.Nat64 }),
  Reverted: IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8), receipt_block_number: IDL.Nat64, confirmed_head_block_number: IDL.Nat64 }),
});
const depositView = IDL.Record({
  deposit_id: IDL.Vec(IDL.Nat8),
  owner_sequence: IDL.Nat64,
  gross_amount: IDL.Nat,
  net_amount: IDL.Nat,
  service_fee: IDL.Nat,
  base_recipient: IDL.Vec(IDL.Nat8),
  state: IDL.Text,
  base_confirmation: IDL.Opt(baseConfirmation),
  last_settlement_stop_reason: IDL.Opt(IDL.Text),
  next_automatic_confirmation_check_at_ns: IDL.Opt(IDL.Nat64),
});
const withdrawalView = IDL.Record({ withdrawal_id: IDL.Vec(IDL.Nat8), amount: IDL.Nat, min_amount_out: IDL.Nat, state: IDL.Text, base_confirmation: IDL.Opt(baseConfirmation), last_settlement_stop_reason: IDL.Opt(IDL.Text), next_automatic_confirmation_check_at_ns: IDL.Opt(IDL.Nat64) });
const reserveStatus = IDL.Record({ eth_balance_wei: IDL.Nat, cycles_balance: IDL.Nat, required_eth_wei: IDL.Nat, required_cycles: IDL.Nat, eth_surplus_wei: IDL.Nat, cycles_surplus: IDL.Nat, sufficient: IDL.Bool });
const confirmationSchedulerStatus = IDL.Record({ healthy: IDL.Bool, scheduled_operations: IDL.Nat64, next_check_at_ns: IDL.Opt(IDL.Nat64), last_run_ns: IDL.Nat64, last_error: IDL.Opt(IDL.Text) });
const bridgeStatus = IDL.Record({ schema_version: IDL.Nat16, last_safe_base_block: IDL.Nat64, last_reserve_observation_ns: IDL.Nat64, last_safe_observation_ns: IDL.Nat64, counts: IDL.Record({ deposits: IDL.Nat64, withdrawals: IDL.Nat64, pending_ledger_operations: IDL.Nat64, pending_evm_operations: IDL.Nat64, reconciliation_holds: IDL.Nat64, reserved_deposit_mint_amount: IDL.Nat, reserved_deposit_mint_operations: IDL.Nat64, reverted_evm_operations: IDL.Nat64, active_evm_payloads: IDL.Nat64, retained_audit_events: IDL.Nat64, pruned_audit_events: IDL.Nat64, retained_deposit_index_entries: IDL.Nat64 }), reserve: reserveStatus, deposits_paused: IDL.Bool, last_audit_sequence: IDL.Opt(IDL.Nat64), confirmation_scheduler: confirmationSchedulerStatus });
const adminError = IDL.Variant({ Busy: IDL.Null, Unauthorized: IDL.Null, InvalidArgument: IDL.Text, StorageFailure: IDL.Null, InsufficientFeeReserve: IDL.Null, UnresolvedEvmRevert: IDL.Null });
const auditedEvmOperationKind = IDL.Variant({ RefundWithdrawal: IDL.Null, MintDeposit: IDL.Null, CancelRelease: IDL.Null, AcknowledgeRelease: IDL.Null });
const feeRecipientConfig = IDL.Record({ owner: IDL.Principal, subaccount: IDL.Vec(IDL.Nat8) });
const auditEventKind = IDL.Variant({
  RuntimeAdministratorsRotated: IDL.Null,
  EvmOperationReverted: IDL.Record({ confirmed_head_block_number: IDL.Nat64, transaction_hash: IDL.Vec(IDL.Nat8), kind: auditedEvmOperationKind, operation_id: IDL.Nat64 }),
  DepositsPauseRepeated: IDL.Null,
  FeeRecipientChanged: IDL.Record({ previous: feeRecipientConfig, current: feeRecipientConfig }),
  DepositsPaused: IDL.Null,
  DepositsResumed: IDL.Null,
  FeePayoutRequested: IDL.Record({ amount: IDL.Nat }),
  ReserveGateChanged: IDL.Record({ sufficient: IDL.Bool }),
});
const auditEvent = IDL.Record({ timestamp_ns: IDL.Nat64, kind: auditEventKind, caller: IDL.Principal, sequence: IDL.Nat64 });
const auditEventPage = IDL.Record({ events: IDL.Vec(auditEvent), oldest_available_sequence: IDL.Nat64, next_sequence: IDL.Opt(IDL.Nat64), pruned_count: IDL.Nat64, pruned_through_sequence: IDL.Opt(IDL.Nat64), pruned_digest: IDL.Vec(IDL.Nat8) });
const payoutState = IDL.Variant({ Pending: IDL.Null, ReconciliationHold: IDL.Null, Succeeded: IDL.Record({ block_index: IDL.Nat }), Failed: IDL.Null });
const payoutReceipt = IDL.Record({ id: IDL.Nat64, amount: IDL.Nat, state: payoutState });
const publicConfig = IDL.Record({ base_chain_id: IDL.Nat64, bridge_contract: IDL.Vec(IDL.Nat8), ledger_canister_id: IDL.Principal, index_canister_id: IDL.Principal, evm_rpc_canister_id: IDL.Principal, rpc_provider_urls_sha256: IDL.Vec(IDL.Nat8), schema_version: IDL.Nat16, expected_bridge_signer: IDL.Vec(IDL.Nat8) });
const listDepositIdsArgs = IDL.Record({ owner: IDL.Principal, before_cursor: IDL.Opt(IDL.Nat64), limit: IDL.Nat16 });
const depositIdPage = IDL.Record({ deposit_ids: IDL.Vec(IDL.Vec(IDL.Nat8)), next_cursor: IDL.Opt(IDL.Nat64), oldest_available_cursor: IDL.Opt(IDL.Nat64), history_truncated: IDL.Bool });
const consentMetadata = IDL.Record({ utc_offset_minutes: IDL.Opt(IDL.Int16), language: IDL.Text });
const consentRequest = IDL.Record({
  arg: IDL.Vec(IDL.Nat8),
  method: IDL.Text,
  user_preferences: IDL.Record({
    metadata: consentMetadata,
    device_spec: IDL.Opt(IDL.Variant({ GenericDisplay: IDL.Null, FieldsDisplay: IDL.Null })),
  }),
});
const consentResponse = IDL.Variant({
  Ok: IDL.Record({ metadata: consentMetadata, consent_message: IDL.Variant({ GenericDisplayMessage: IDL.Text }) }),
  Err: IDL.Variant({
    GenericError: IDL.Record({ description: IDL.Text, error_code: IDL.Nat }),
    InsufficientPayment: IDL.Record({ description: IDL.Text }),
    UnsupportedCanisterCall: IDL.Record({ description: IDL.Text }),
    ConsentMessageUnavailable: IDL.Record({ description: IDL.Text }),
  }),
});
const bridgeIdl = ({ IDL: I }: { IDL: typeof IDL }) =>
  I.Service({
    request_deposit: I.Func(
      [depositArgs],
      [I.Variant({ Ok: depositReceipt, Err: depositError })],
      [],
    ),
    get_deposit: I.Func([I.Vec(I.Nat8)], [I.Opt(depositView)], ["query"]),
    get_withdrawal: I.Func([I.Vec(I.Nat8)], [I.Opt(withdrawalView)], ["query"]),
    notify_withdrawal: I.Func([I.Record({ transaction_hash: I.Vec(I.Nat8) })], [I.Variant({ Ok: I.Variant({ Duplicate: I.Record({ withdrawal_id: I.Vec(I.Nat8), settlement: I.Opt(settlementAction) }), Ingested: I.Record({ confirmed_head_block_number: I.Nat64, withdrawal_id: I.Vec(I.Nat8), settlement: I.Opt(settlementAction) }) }), Err: I.Variant({ Busy: I.Null, RpcUnavailable: I.Null, TransactionNotConfirmed: I.Null, WithdrawalConflict: I.Null, OwnerMismatch: I.Null, RpcInconsistent: I.Null, RateLimited: I.Record({ retry_after_seconds: I.Nat64 }), InvalidTransactionHash: I.Null, TransactionReverted: I.Null, LedgerFeeUnavailable: I.Null, StorageFailure: I.Null, TransactionNotFound: I.Null, AnonymousCaller: I.Null, InvalidBaseResponse: I.Null, BaseStateMismatch: I.Null, BridgeSignerMismatch: I.Null }) })], []),
    continue_deposit: I.Func([I.Vec(I.Nat8)], [I.Variant({ Ok: settlementAction, Err: settlementActionError })], []),
    continue_withdrawal: I.Func([I.Vec(I.Nat8)], [I.Variant({ Ok: settlementAction, Err: settlementActionError })], []),
    continue_fee_payout: I.Func([I.Nat64], [I.Variant({ Ok: settlementAction, Err: settlementActionError })], []),
    get_bridge_status: I.Func([], [bridgeStatus], ["query"]),
    get_public_config: I.Func([], [publicConfig], []),
    get_next_deposit_sequence: I.Func([I.Principal], [I.Nat64], ["query"]),
    list_deposit_ids: I.Func([listDepositIdsArgs], [I.Variant({ Ok: depositIdPage, Err: I.Variant({ InvalidLimit: I.Null }) })], ["query"]),
    icrc10_supported_standards: I.Func([], [I.Vec(I.Record({ name: I.Text, url: I.Text }))], ["query"]),
    icrc21_canister_call_consent_message: I.Func([consentRequest], [consentResponse], []),
    pause_new_deposits: I.Func([], [I.Variant({ Ok: I.Null, Err: adminError })], []),
    resume_new_deposits: I.Func([], [I.Variant({ Ok: I.Null, Err: adminError })], []),
    rotate_runtime_administrators: I.Func([I.Record({ pause_principals: I.Vec(I.Principal), finance_administrator: I.Principal })], [I.Variant({ Ok: I.Null, Err: adminError })], []),
    get_audit_events: I.Func([I.Nat64, I.Nat16], [I.Variant({ Ok: auditEventPage, Err: adminError })], ["query"]),
    request_fee_payout: I.Func([I.Nat], [I.Variant({ Ok: payoutReceipt, Err: adminError })], []),
  });
describe("Phase 3 PocketIC saga", () => {
  let server: ChildProcess | undefined;
  let pic: PocketIc | undefined;
  let serverUrl = "";

  async function setup(activate = true) {
    const mockBytes = readFileSync(mockWasm);
    const subnet = await pic!.getFiduciarySubnet();
    if (subnet === undefined) throw new Error("Fiduciary subnet was not created");
    const installMock = (ledgerId: Principal) => pic!.setupCanister({ idlFactory: mockIdl, wasm: mockBytes, arg: IDL.encode([mockInit], [{ ledger_id: ledgerId }]), cycles: 50_000_000_000_000n, targetSubnetId: subnet.id });
    const ledger = await installMock(Principal.anonymous());
    const index = await installMock(ledger.canisterId);
    const evm = await installMock(ledger.canisterId);
    const missing: any = await (evm.actor as any).probe_chain_key("missing_test_key");
    expect(missing.Err).toContain("unknown threshold key");
    const preflight: any = await (evm.actor as any).probe_chain_key("key_1");
    if (!("Ok" in preflight)) {
      throw new Error(`chain-key preflight failed: key=key_1 subnet=${subnet.id.toText()} error=${preflight.Err}`);
    }
    expect(preflight.Ok.public_key).toHaveLength(33);
    expect(preflight.Ok.signature).toHaveLength(64);
    const runtimePrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(7));
    const init = { ledger_canister_id: ledger.canisterId, index_canister_id: index.canisterId, evm_rpc_canister_id: evm.canisterId, custom_evm_rpc_urls: [], base_chain_id: 8453n, bridge_contract: new Uint8Array(20).fill(1), ecdsa_key_name: "key_1", ecdsa_derivation_path: [], deposit_rate_limit_window_seconds: 60n, deposit_rate_limit_global: 30, deposit_rate_limit_per_principal: 3, settlement_rate_limit_window_seconds: 600n, settlement_rate_limit_global: 60, settlement_rate_limit_per_principal: 6, settlement_rate_limit_per_record: 3, transaction_gas_limit: 500_000n, max_fee_per_gas: 10n, max_priority_fee_per_gas: 1n, eth_floor_wei: 1n, cycles_floor: 1n, settlement_cycle_ceiling: 1n, governance_principal: runtimePrincipal, pause_principals: [runtimePrincipal], finance_administrator: runtimePrincipal, fee_recipient: { owner: runtimePrincipal, subaccount: [] } };
    const bridge = await pic!.setupCanister({ idlFactory: bridgeIdl, wasm: readFileSync(bridgeWasm), arg: IDL.encode([bridgeInit], [init]), cycles: 500_000_000_000_000n, targetSubnetId: subnet.id });
    bridge.actor.setPrincipal(runtimePrincipal);
    const configuredSigner: any = await (evm.actor as any).set_bridge_signer_for_canister(bridge.canisterId, init.ecdsa_key_name);
    if (!("Ok" in configuredSigner)) throw new Error(`failed to configure mock bridge signer: ${configuredSigner.Err}`);
    if (activate) expect(await (bridge.actor as any).resume_new_deposits()).toHaveProperty("Ok");
    expect((await pic!.getCanisterSubnetId(bridge.canisterId))?.toText()).toBe(subnet.id.toText());
    return { ledger, index, evm, bridge, init, runtimePrincipal };
  }

  async function advanceTimeWithoutSettlement(rounds = 5) { for (let step = 0; step < rounds; step += 1) { await pic!.advanceTime(60_000); await pic!.tick(5); } }
  async function advanceAutomaticConfirmation(minutes: number) {
    await pic!.advanceTime(minutes * 60_000);
    await pic!.tick(30);
  }
  async function notifyFixtureWithdrawal(bridge: any) {
    const result = await bridge.actor.notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    expect(result).toHaveProperty("Ok");
    expect(result.Ok).toHaveProperty("Ingested");
    return result;
  }

  beforeAll(async () => {
    const probe = createServer();
    const port = await new Promise<number>((resolvePort, reject) => {
      probe.once("error", reject);
      probe.listen(0, "127.0.0.1", () => {
        const address = probe.address();
        if (typeof address === "string" || address === null) reject(new Error("no test port"));
        else probe.close(() => resolvePort(address.port));
      });
    });
    serverUrl = `http://127.0.0.1:${port}`;
    server = spawn(resolve("node_modules/@dfinity/pic/pocket-ic"), ["--port", String(port), "--hard-ttl", "600"], { stdio: "inherit" });
    for (let attempt = 0; attempt < 100; attempt += 1) {
      try {
        await fetch(serverUrl);
        return;
      } catch {
        await new Promise((resolveReady) => setTimeout(resolveReady, 100));
      }
    }
    throw new Error("PocketIC server did not become ready");
  });

  afterAll(async () => {
    server?.kill();
  });

  beforeEach(async () => {
    if (server === undefined) throw new Error("PocketIC server was not started");
    pic = await PocketIc.create(serverUrl, {
      nns: { state: { type: SubnetStateType.New } },
      fiduciary: { state: { type: SubnetStateType.New } },
    });
  });

  afterEach(async () => {
    await pic?.tearDown();
  });

  it("persists one idempotent Deposit through ledger pull, EVM submission, and Safe-confirmed mint", async () => {
    const { bridge } = await setup();

    const request = {
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 100n,
      max_service_fee: 10n,
    };
    const first: any = await (bridge.actor as any).request_deposit(request);
    if (!("Ok" in first)) {
      throw new Error(`request_deposit failed: ${JSON.stringify(first)}`);
    }
    expect(first.Ok.state).toBe("MintPending");
    await advanceAutomaticConfirmation(20);
    const replay: any = await (bridge.actor as any).request_deposit(request);
    expect(Array.from(replay.Ok.deposit_id)).toEqual(Array.from(first.Ok.deposit_id));

    const stored: any = await (bridge.actor as any).get_deposit(first.Ok.deposit_id);
    expect(stored[0].state).toBe("Minted");
    for (let upgrade = 0; upgrade < 2; upgrade += 1) {
      await pic!.upgradeCanister({
        canisterId: bridge.canisterId,
        wasm: readFileSync(bridgeWasm),
        arg: IDL.encode([], []),
      });
      const reopened: any = await (bridge.actor as any).get_deposit(first.Ok.deposit_id);
      expect(reopened[0].state).toBe("Minted");
      const replayAfterUpgrade: any = await (bridge.actor as any).request_deposit(request);
      expect(Array.from(replayAfterUpgrade.Ok.deposit_id)).toEqual(Array.from(first.Ok.deposit_id));
    }
  });

  it("uses a stable owner sequence for deterministic replay, conflicts, and gaps", async () => {
    const { bridge, runtimePrincipal } = await setup();
    expect(await (bridge.actor as any).get_next_deposit_sequence(runtimePrincipal)).toBe(0n);
    const request = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n };
    const first: any = await (bridge.actor as any).request_deposit(request);
    expect(first).toHaveProperty("Ok");
    expect(first.Ok.owner_sequence).toBe(0n);
    expect(await (bridge.actor as any).get_next_deposit_sequence(runtimePrincipal)).toBe(1n);
    const replay: any = await (bridge.actor as any).request_deposit(request);
    expect(Array.from(replay.Ok.deposit_id)).toEqual(Array.from(first.Ok.deposit_id));
    expect(await (bridge.actor as any).request_deposit({ ...request, gross_amount: 101n })).toEqual({ Err: { DepositConflict: null } });
    expect(await (bridge.actor as any).request_deposit({ ...request, owner_sequence: 2n })).toEqual({ Err: { SequenceMismatch: { expected: 1n } } });
  });

  it("authorizes owners and settlement administrators but rejects anonymous and third-party Continue calls", async () => {
    const { bridge, runtimePrincipal } = await setup();
    const owner = Principal.selfAuthenticating(new Uint8Array(32).fill(31));
    const thirdParty = Principal.selfAuthenticating(new Uint8Array(32).fill(32));
    const pausePrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(33));
    const deposit = async (tag: number) => (bridge.actor as any).request_deposit({ owner_sequence: BigInt(tag - 91), base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });

    bridge.actor.setPrincipal(owner);
    const ownerDeposit: any = await deposit(91);
    await advanceAutomaticConfirmation(20);
    expect(await (bridge.actor as any).continue_deposit(ownerDeposit.Ok.deposit_id)).toHaveProperty("Ok.Complete");

    const governanceDeposit: any = await deposit(92);
    await advanceAutomaticConfirmation(20);
    bridge.actor.setPrincipal(thirdParty);
    expect(await (bridge.actor as any).continue_deposit(governanceDeposit.Ok.deposit_id)).toEqual({ Err: { Unauthorized: null } });
    bridge.actor.setPrincipal(Principal.anonymous());
    expect(await (bridge.actor as any).continue_deposit(governanceDeposit.Ok.deposit_id)).toEqual({ Err: { AnonymousCaller: null } });
    bridge.actor.setPrincipal(runtimePrincipal);
    expect(await (bridge.actor as any).continue_deposit(governanceDeposit.Ok.deposit_id)).toHaveProperty("Ok.Complete");

    expect(await (bridge.actor as any).rotate_runtime_administrators({ pause_principals: [pausePrincipal], finance_administrator: runtimePrincipal })).toHaveProperty("Ok");
    bridge.actor.setPrincipal(owner);
    const pauseDeposit: any = await deposit(93);
    await advanceAutomaticConfirmation(20);
    bridge.actor.setPrincipal(pausePrincipal);
    expect(await (bridge.actor as any).continue_deposit(pauseDeposit.Ok.deposit_id)).toHaveProperty("Ok.Complete");
  });

  it("allows only one concurrent Continue call for the same in-flight record", async () => {
    const { ledger, bridge, runtimePrincipal } = await setup();
    await (ledger.actor as any).set_ledger_mode({ TemporarilyUnavailable: null });
    const deposit: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    const deferred = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    deferred.setPrincipal(runtimePrincipal);
    const first = await deferred.continue_deposit(deposit.Ok.deposit_id);
    const second = await deferred.continue_deposit(deposit.Ok.deposit_id);
    const results: any[] = await Promise.all([first(), second()]);
    expect(results.filter((result) => "Ok" in result && "Submitted" in result.Ok)).toHaveLength(1);
    expect(results.filter((result) => "Err" in result && "Busy" in result.Err)).toHaveLength(1);
  });

  it("allows unrelated records to Continue concurrently without Busy", async () => {
    const { ledger, bridge, runtimePrincipal } = await setup();
    await (ledger.actor as any).set_ledger_mode({ TemporarilyUnavailable: null });
    const first: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    const second: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(5), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    expect(first).toHaveProperty("Ok");
    expect(second).toHaveProperty("Ok");
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    const deferred = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    deferred.setPrincipal(runtimePrincipal);
    const continueFirst = await deferred.continue_deposit(first.Ok.deposit_id);
    const continueSecond = await deferred.continue_deposit(second.Ok.deposit_id);
    const results: any[] = await Promise.all([continueFirst(), continueSecond()]);
    expect(results.every((result) => "Ok" in result)).toBe(true);
    expect(results.some((result) => "Err" in result && "Busy" in result.Err)).toBe(false);
  });

  it("binds a selected subaccount, exposes public configuration, consent, and owner history", async () => {
    const { bridge, init, runtimePrincipal } = await setup();
    const selectedSubaccount = new Uint8Array(32).fill(8);
    const request = {
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(9),
      from_subaccount: [selectedSubaccount],
      gross_amount: 100n,
      max_service_fee: 10n,
    };
    const depositIdl = ({ IDL: I }: { IDL: typeof IDL }) => I.Service({
      request_deposit: I.Func([depositArgs], [I.Variant({ Ok: depositReceipt, Err: depositError })], []),
    });
    const depositActor = pic!.createActor(depositIdl, bridge.canisterId);
    depositActor.setPrincipal(runtimePrincipal);

    const standards: any = await (bridge.actor as any).icrc10_supported_standards();
    expect(standards).toEqual([{ name: "ICRC-21", url: "https://github.com/dfinity/ICRC/blob/main/ICRCs/ICRC-21/ICRC-21.md" }]);
    const config: any = await (bridge.actor as any).get_public_config();
    expect(config.base_chain_id).toBe(8453n);
    expect(config.schema_version).toBe(6);
    expect(config.ledger_canister_id.toText()).toBe(init.ledger_canister_id.toText());
    expect(config.evm_rpc_canister_id.toText()).toBe(init.evm_rpc_canister_id.toText());
    expect(config.rpc_provider_urls_sha256).toHaveLength(32);

    const consent: any = await (bridge.actor as any).icrc21_canister_call_consent_message({
      arg: new Uint8Array(IDL.encode([depositArgs], [request])),
      method: "request_deposit",
      user_preferences: { metadata: { utc_offset_minutes: [], language: "en" }, device_spec: [{ GenericDisplay: null }] },
    });
    expect(consent.Ok.consent_message.GenericDisplayMessage).toContain("Source subaccount: `0x0808");
    expect(consent.Ok.consent_message.GenericDisplayMessage).toContain("does not provide SNS voting rights");

    const statusBeforeWithdrawalConsent = await (bridge.actor as any).get_bridge_status();
    const withdrawalConsent: any = await (bridge.actor as any).icrc21_canister_call_consent_message({
      arg: new Uint8Array(IDL.encode([IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8) })], [{ transaction_hash: new Uint8Array(32).fill(6) }])),
      method: "notify_withdrawal",
      user_preferences: { metadata: { utc_offset_minutes: [], language: "en" }, device_spec: [{ GenericDisplay: null }] },
    });
    expect(withdrawalConsent.Ok.consent_message.GenericDisplayMessage).toContain("Base transaction: `0x0606");
    expect(withdrawalConsent.Ok.consent_message.GenericDisplayMessage).toContain("Base chain ID: `8453`");
    expect(withdrawalConsent.Ok.consent_message.GenericDisplayMessage).toContain("Bridge contract: `0x0101");
    expect(withdrawalConsent.Ok.consent_message.GenericDisplayMessage).toContain("does not guarantee settlement success");
    const statusAfterWithdrawalConsent = await (bridge.actor as any).get_bridge_status();
    const withoutLiveCycles = (status: any) => ({ ...status, reserve: { ...status.reserve, cycles_balance: 0n, cycles_surplus: 0n } });
    expect(withoutLiveCycles(statusAfterWithdrawalConsent)).toEqual(withoutLiveCycles(statusBeforeWithdrawalConsent));

    const malformedWithdrawalConsent: any = await (bridge.actor as any).icrc21_canister_call_consent_message({
      arg: new Uint8Array(IDL.encode([IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8) })], [{ transaction_hash: new Uint8Array(31) }])),
      method: "notify_withdrawal",
      user_preferences: { metadata: { utc_offset_minutes: [], language: "en" }, device_spec: [] },
    });
    expect(malformedWithdrawalConsent).toHaveProperty("Err.ConsentMessageUnavailable");

    const anonymousBridge = pic!.createActor(bridgeIdl, bridge.canisterId);
    const anonymousWithdrawalConsent: any = await (anonymousBridge as any).icrc21_canister_call_consent_message({
      arg: new Uint8Array(IDL.encode([IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8) })], [{ transaction_hash: new Uint8Array(32).fill(6) }])),
      method: "notify_withdrawal",
      user_preferences: { metadata: { utc_offset_minutes: [], language: "en" }, device_spec: [] },
    });
    expect(anonymousWithdrawalConsent).toHaveProperty("Err.ConsentMessageUnavailable");

    const accepted: any = await (depositActor as any).request_deposit(request);
    expect(accepted).toHaveProperty("Ok");
    const page: any = await (bridge.actor as any).list_deposit_ids({ owner: runtimePrincipal, before_cursor: [], limit: 20 });
    expect(Array.from(page.Ok.deposit_ids[0])).toEqual(Array.from(accepted.Ok.deposit_id));
    expect(page.Ok.next_cursor).toEqual([]);

    const conflict: any = await (depositActor as any).request_deposit({ ...request, from_subaccount: [new Uint8Array(32).fill(7)] });
    expect(conflict).toHaveProperty("Err.DepositConflict");
    const replayPage: any = await (bridge.actor as any).list_deposit_ids({ owner: runtimePrincipal, before_cursor: [], limit: 20 });
    expect(replayPage.Ok.deposit_ids).toHaveLength(1);
  });

  it("freezes the accepted service fee across a later Base fee change", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
    expect(result).toHaveProperty("Ok");
    await (evm.actor as any).set_service_fee(7n);
    await advanceAutomaticConfirmation(20);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(stored[0].service_fee).toBe(1n);
    expect(stored[0].net_amount).toBe(99n);
    expect(stored[0].state).toBe("Minted");
  });

  it("reserves pending Mint capacity and rejects overflow before ledger pull", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    await (evm.actor as any).set_mint_window(90n, 100n, 0n, 100n, 1n);
    const first: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    expect(first).toHaveProperty("Ok");
    const second: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 3n, max_service_fee: 10n });
    expect(second).toHaveProperty("Err.Rejected");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
    const status: any = await (bridge.actor as any).get_bridge_status();
    expect(status.counts.reserved_deposit_mint_amount).toBe(9n);
    expect(status.counts.reserved_deposit_mint_operations).toBe(1n);
  });

  it("atomically admits only one concurrent Deposit when reserve covers one candidate", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_eth_balance(0n);
    const warmup: any = await (bridge.actor as any).request_deposit({
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 100n,
      max_service_fee: 10n,
    });
    expect(warmup).toEqual({ Err: { ReserveUnavailable: null } });
    await (evm.actor as any).set_eth_balance(5_000_001n);
    const firstPrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(0x31));
    const secondPrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(0x32));
    const firstActor = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    const secondActor = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    firstActor.setPrincipal(firstPrincipal);
    secondActor.setPrincipal(secondPrincipal);
    const args = {
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 100n,
      max_service_fee: 10n,
    };

    const first = await firstActor.request_deposit(args);
    const second = await secondActor.request_deposit(args);
    const results: any[] = await Promise.all([first(), second()]);
    expect(results.filter((result) => "Ok" in result)).toHaveLength(1);
    expect(results.filter((result) => "Err" in result && "ReserveUnavailable" in result.Err)).toHaveLength(1);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    const status: any = await (bridge.actor as any).get_bridge_status();
    expect(status.counts.reserved_deposit_mint_operations).toBe(1n);
  });

  it("treats a full expired Mint window as having zero effective consumption", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_mint_window(100n, 100n, 0n, 10n, 10n);
    const accepted: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    expect(accepted).toHaveProperty("Ok");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("refreshes at most one stale Mint snapshot per request and fails closed", async () => {
    const { ledger, evm, bridge } = await setup();
    const seed: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    await advanceAutomaticConfirmation(20);
    expect((await (bridge.actor as any).get_deposit(seed.Ok.deposit_id))[0].state).toBe("Minted");

    await pic!.advanceTime(61_000);
    await (evm.actor as any).set_safe_block_sequence([98n, 100n]);
    const stale: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    expect(stale).toEqual({ Err: { BaseObservationUnavailable: null } });
    await pic!.advanceTime(61_000);
    const refreshed: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    expect(refreshed).toHaveProperty("Ok");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(2);

    await pic!.advanceTime(61_000);
    await (evm.actor as any).set_safe_block_sequence([98n, 98n, 98n, 98n, 98n]);
    const unavailable: any = await (bridge.actor as any).request_deposit({ owner_sequence: 2n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    expect(unavailable).toEqual({ Err: { BaseObservationUnavailable: null } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(2);
  });

  it("reuses a safe Base Mint snapshot within the admission TTL", async () => {
    const { evm, bridge } = await setup();
    const before = await (evm.actor as any).eth_call_count();
    expect(await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n })).toHaveProperty("Ok");
    const afterFirst = await (evm.actor as any).eth_call_count();
    expect(await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n })).toHaveProperty("Ok");
    const afterSecond = await (evm.actor as any).eth_call_count();
    expect(afterFirst - before).toBe(1n);
    expect(afterSecond - afterFirst).toBe(0n);
  });

  it("rejects a Base deposit-mint pause before pulling funds from the ledger", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_deposit_mints_paused(true);
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    expect(result).toEqual({ Err: { DepositsPaused: null } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);
  });

  it("rejects a rotated Base bridge signer before pulling funds from the ledger", async () => {
    const { ledger, evm, bridge } = await setup();
    expect(await (evm.actor as any).set_bridge_signer(new Uint8Array(20).fill(0xaa))).toHaveProperty("Ok");
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    expect(result).toEqual({ Err: { BaseObservationUnavailable: null } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);
  });

  it("rate-limits new deposit admissions while preserving idempotent retries", async () => {
    const { bridge } = await setup();
    const request = (tag: number) => ({ owner_sequence: BigInt(tag - 72), base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    const first: any = await (bridge.actor as any).request_deposit(request(72));
    expect(first).toHaveProperty("Ok");
    expect(await (bridge.actor as any).request_deposit(request(73))).toHaveProperty("Ok");
    expect(await (bridge.actor as any).request_deposit(request(74))).toHaveProperty("Ok");
    const limited: any = await (bridge.actor as any).request_deposit(request(75));
    expect(limited.Err.RateLimited.retry_after_seconds).toBeGreaterThan(0n);
    expect(limited.Err.RateLimited.retry_after_seconds).toBeLessThanOrEqual(60n);
    const replay: any = await (bridge.actor as any).request_deposit(request(72));
    expect(Array.from(replay.Ok.deposit_id)).toEqual(Array.from(first.Ok.deposit_id));
  });

  it("rechecks pause atomically after request awaits while preserving idempotent retries", async () => {
    const { ledger, bridge, runtimePrincipal } = await setup();
    const args = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n };
    const deferred = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    deferred.setPrincipal(runtimePrincipal);
    const awaitDeposit = await deferred.request_deposit(args);
    const awaitPause = await deferred.pause_new_deposits();
    const [pending] = await Promise.all([awaitDeposit(), awaitPause()]);
    expect(pending).toEqual({ Err: { DepositsPaused: null } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);

    await (bridge.actor as any).resume_new_deposits();
    const accepted: any = await (bridge.actor as any).request_deposit(args);
    expect(accepted).toHaveProperty("Ok");
    await (bridge.actor as any).pause_new_deposits();
    const replay: any = await (bridge.actor as any).request_deposit(args);
    expect(Array.from(replay.Ok.deposit_id)).toEqual(Array.from(accepted.Ok.deposit_id));
  });

  it("accepts a safe withdrawal, releases ICP in the notification call, and confirms acknowledgement", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    await (evm.actor as any).set_next_evm_nonce(7n);
    const id = new Uint8Array(32).fill(6);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    const ingested = await notifyFixtureWithdrawal(bridge);
    expect(Array.from(ingested.Ok.Ingested.withdrawal_id)).toEqual(Array.from(id));
    expect((await (bridge.actor as any).get_withdrawal(id))[0].state).toBe("AcknowledgePending");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    await (evm.actor as any).set_observed_transaction(new Uint8Array(32).fill(9), new Uint8Array(20).fill(1), new Uint8Array(20).fill(0x22), 99n);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    const duplicate: any = await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    expect(Array.from(duplicate.Ok.Duplicate.withdrawal_id)).toEqual(Array.from(id));
    await (evm.actor as any).set_withdrawal_status(3);
    await advanceAutomaticConfirmation(20);
    expect((await (bridge.actor as any).get_bridge_status()).counts.withdrawals).toBe(1n);
    const withdrawal: any = await (bridge.actor as any).get_withdrawal(id);
    expect(withdrawal[0].state).toBe("Released");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
    const broadcasts = await (evm.actor as any).broadcast_transactions();
    expect(broadcasts).toHaveLength(1);
  });

  it("never calls the Ledger before the user withdrawal reaches the safe head", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xa0);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    await (evm.actor as any).set_safe_block_sequence([98n]);
    const premature: any = await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    expect(premature).toHaveProperty("Err.TransactionNotConfirmed");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    await (evm.actor as any).set_safe_block_sequence([100n]);
    const notified: any = await notifyFixtureWithdrawal(bridge);
    expect(notified.Ok.Ingested.settlement[0]).toHaveProperty("Submitted");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    expect((await (bridge.actor as any).get_withdrawal(id))[0].state).toBe("AcknowledgePending");
  });

  it("returns notification observation failures immediately and never retries them from timers", async () => {
    const cases = [
      [{ Missing: null }, "TransactionNotFound"],
      [{ Reverted: null }, "TransactionReverted"],
      [{ RpcFailure: null }, "RpcUnavailable"],
      [{ Inconsistent: null }, "RpcInconsistent"],
      [{ DecodeFailure: null }, "InvalidBaseResponse"],
      [{ Orphaned: null }, "InvalidBaseResponse"],
    ] as const;
    for (const [mode, error] of cases) {
      const { evm, bridge, runtimePrincipal } = await setup();
      const id = new Uint8Array(32).fill(80 + cases.findIndex(([candidate]) => candidate === mode));
      await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
      await (evm.actor as any).set_receipt_mode(mode);
      const result: any = await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
      expect(result).toHaveProperty(`Err.${error}`);
      await advanceTimeWithoutSettlement(2);
      expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
      await pic!.tearDown();
      pic = await PocketIc.create(serverUrl, { nns: { state: { type: SubnetStateType.New } }, fiduciary: { state: { type: SubnetStateType.New } } });
    }
  });

  it("binds withdrawal state reads to the canonical receipt block with EIP-1898", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0x9a);
    await (evm.actor as any).set_observed_transaction(
      new Uint8Array(32).fill(9),
      new Uint8Array(20).fill(1),
      new Uint8Array(20).fill(0x22),
      99n,
    );
    await (evm.actor as any).set_withdrawal([{
      id,
      owner: runtimePrincipal.toUint8Array(),
      subaccount: new Uint8Array(32),
      amount: 100n,
      min_amount_out: 90n,
    }]);

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) }))
      .toHaveProperty("Ok.Ingested");
    expect(Array.from(await (evm.actor as any).pinned_eth_call_block_numbers())).toEqual([99n, 99n, 100n]);
  });

  it.each([
    { mode: { Wrong: null }, error: "BaseStateMismatch" },
    { mode: { Inconsistent: null }, error: "RpcInconsistent" },
  ])("fails closed on $error eth_chainId observations before Ledger", async ({ mode, error }) => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(error === "BaseStateMismatch" ? 0x9b : 0x9c);
    await (evm.actor as any).set_withdrawal([{
      id,
      owner: runtimePrincipal.toUint8Array(),
      subaccount: new Uint8Array(32),
      amount: 100n,
      min_amount_out: 90n,
    }]);
    await (evm.actor as any).set_chain_id_mode(mode);

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) }))
      .toHaveProperty(`Err.${error}`);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(Array.from(await (evm.actor as any).pinned_eth_call_block_numbers())).toEqual([]);
  });

  it.each([
    { mode: { SafeInconsistent: null }, error: "RpcInconsistent", tag: 0x9e },
    { mode: { CanonicalInconsistent: null }, error: "RpcInconsistent", tag: 0x9f },
    { mode: { SameHeightDifferentHash: null }, error: "InvalidBaseResponse", tag: 0x9d },
  ])("fails closed on $error canonical block observations before Ledger", async ({ mode, error, tag }) => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(tag);
    await (evm.actor as any).set_withdrawal([{
      id,
      owner: runtimePrincipal.toUint8Array(),
      subaccount: new Uint8Array(32),
      amount: 100n,
      min_amount_out: 90n,
    }]);
    await (evm.actor as any).set_block_mode(mode);

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) }))
      .toHaveProperty(`Err.${error}`);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  });

  it("rejects a refunded old receipt before any Ledger release call", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xa1);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    await (evm.actor as any).set_withdrawal_status(4);

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toEqual({ Err: { BaseStateMismatch: null } });
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  });

  it("rejects signer rotation between the receipt and Safe Base state read before Ledger", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xa2);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    expect(await (evm.actor as any).set_bridge_signer(new Uint8Array(20).fill(0xaa))).toHaveProperty("Ok");

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toEqual({ Err: { BridgeSignerMismatch: null } });
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  });

  it("rejects non-confirmed and wrong-owner notifications and ingests one concurrent replay", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(86);
    await (evm.actor as any).set_withdrawal([{ id, owner: Principal.selfAuthenticating(new Uint8Array(32).fill(8)).toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Err.OwnerMismatch");
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    await (evm.actor as any).set_observed_transaction(new Uint8Array(32).fill(9), new Uint8Array(20).fill(1), new Uint8Array(20).fill(0x22), 101n);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Err.TransactionNotConfirmed");
    await (evm.actor as any).set_observed_transaction(new Uint8Array(32).fill(9), new Uint8Array(20).fill(1), new Uint8Array(20).fill(0x22), 99n);
    const deferred = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    deferred.setPrincipal(runtimePrincipal);
    const first = await deferred.notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    const second = await deferred.notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    const results: any[] = await Promise.all([first(), second()]);
    expect(results.filter((result) => "Ok" in result && "Ingested" in result.Ok)).toHaveLength(1);
    expect(results.filter((result) => "Err" in result && "Busy" in result.Err)).toHaveLength(1);
    expect((await (bridge.actor as any).get_bridge_status()).counts.withdrawals).toBe(1n);
  });

  it("rejects a conflicting payload for an existing withdrawal ID", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(89);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Ok.Ingested");
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 101n, min_amount_out: 90n }]);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Err.WithdrawalConflict");
  });

  it("rate-limits notification attempts before another Base snapshot call", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(88);
    for (let attempt = 0; attempt < 4; attempt += 1) {
      await (evm.actor as any).set_observed_transaction(new Uint8Array(32).fill(9), new Uint8Array(20).fill(1), new Uint8Array(20).fill(0x22), 99n);
      await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
      expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Ok");
    }
    const callsBeforeLimit = await (evm.actor as any).eth_call_count();
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Err.RateLimited");
    expect(await (evm.actor as any).eth_call_count()).toBe(callsBeforeLimit);
  });

  it("returns a ledger fee failure without storing or scheduling the withdrawal", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(87);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    await (ledger.actor as any).set_ledger_fee_available(false);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Err.LedgerFeeUnavailable");
    await advanceTimeWithoutSettlement(2);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  });

  it("continues an ambiguous Withdrawal release from reconciled Hold to acknowledgement", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    const id = new Uint8Array(32).fill(46);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    await notifyFixtureWithdrawal(bridge);
    await advanceAutomaticConfirmation(20);
    const held: any = await (bridge.actor as any).get_withdrawal(id);
    expect(held[0].state).toBe("ReconciliationHold");
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    expect(await (bridge.actor as any).continue_withdrawal(id)).toHaveProperty("Ok.Submitted");
    await advanceAutomaticConfirmation(20);
    const released: any = await (bridge.actor as any).get_withdrawal(id);
    expect(released[0].state).toBe("Released");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it.each([
    { label: "increased", initialFee: 1n, nextFee: 2n },
    { label: "decreased", initialFee: 3n, nextFee: 2n },
  ])("reprices a definitely unsent Withdrawal after a $label BadFee", async ({ initialFee, nextFee }) => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(Number(0xb0n + initialFee));
    await (ledger.actor as any).set_ledger_fee(initialFee);
    await (ledger.actor as any).set_bad_fee_expected_fee([nextFee]);
    await (ledger.actor as any).set_ledger_mode({ BadFee: null });
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    await notifyFixtureWithdrawal(bridge);

    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    const repriced: any = await (bridge.actor as any).get_withdrawal(id);
    expect(repriced[0].state).toBe("ReleasePending");
    expect(repriced[0].last_settlement_stop_reason).toEqual(["Ledger fee changed; settlement identity was updated without sending"]);

    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    expect(await (bridge.actor as any).continue_withdrawal(id)).toHaveProperty("Ok.Submitted");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(2n);
    await advanceAutomaticConfirmation(20);
    expect((await (bridge.actor as any).get_withdrawal(id))[0].state).toBe("Released");
  });

  it("cancels the Base release lock before refund when a BadFee increase breaks minimum", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xb4);
    await (ledger.actor as any).set_ledger_fee(1n);
    await (ledger.actor as any).set_bad_fee_expected_fee([2n]);
    await (ledger.actor as any).set_ledger_mode({ BadFee: null });
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 98n }]);
    await notifyFixtureWithdrawal(bridge);

    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    const cancelled: any = await (bridge.actor as any).get_withdrawal(id);
    expect(cancelled[0].state).toBe("ReleaseCancellationPending");
    expect(cancelled[0].last_settlement_stop_reason).toEqual(["Ledger fee changed; settlement identity was updated without sending"]);
    expect(await (bridge.actor as any).continue_withdrawal(id)).toHaveProperty("Ok.Submitted");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    await advanceAutomaticConfirmation(20);
    expect((await (bridge.actor as any).get_withdrawal(id))[0].state).toBe("RefundPending");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    await advanceAutomaticConfirmation(20);
    expect((await (bridge.actor as any).get_withdrawal(id))[0].state).toBe("Refunded");
  });

  it("does not reprice or cancel after an ambiguous Ledger release", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xb5);
    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    await notifyFixtureWithdrawal(bridge);
    expect((await (bridge.actor as any).get_withdrawal(id))[0].state).toBe("ReconciliationHold");

    await (ledger.actor as any).set_ledger_fee(2n);
    await (ledger.actor as any).set_ledger_mode({ BadFee: null });
    const retry: any = await (bridge.actor as any).continue_withdrawal(id);
    expect(retry).toHaveProperty("Ok.Stopped.reason.LedgerRejected");
    expect((await (bridge.actor as any).get_withdrawal(id))[0].state).toBe("ReconciliationHold");
  });

  it("refunds an uneconomic burn without sending ICP", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(9);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 2n, min_amount_out: 2n }]);
    await notifyFixtureWithdrawal(bridge);
    await advanceAutomaticConfirmation(20);
    await advanceAutomaticConfirmation(20);
    const withdrawal: any = await (bridge.actor as any).get_withdrawal(id);
    expect(withdrawal[0].state).toBe("Refunded");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);
    const broadcasts = await (evm.actor as any).broadcast_transactions();
    expect(broadcasts).toHaveLength(2);
    expect(Buffer.from(broadcasts[0]).equals(Buffer.from(broadcasts[1]))).toBe(false);
  });

  it("continues an ambiguous deposit from reconciled Hold to Mint", async () => {
    const { ledger, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
    expect(result.Ok.state).toBe("ReconciliationHold");
    const before: any = await (bridge.actor as any).get_bridge_status();
    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    const after: any = await (bridge.actor as any).get_bridge_status();
    expect(after.counts.reconciliation_holds).toBe(before.counts.reconciliation_holds);
    expect(after.counts.reconciliation_holds).toBe(1n);
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    expect(await (bridge.actor as any).continue_deposit(result.Ok.deposit_id)).toHaveProperty("Ok.Submitted");
    await advanceAutomaticConfirmation(20);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(stored[0].state).toBe("Minted");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("does not retry a retryable deposit pull in the admission call", async () => {
    const { ledger, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode({ TemporarilyUnavailable: null });

    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });

    expect(result).toHaveProperty("Ok.settlement.0.Stopped");
    expect(result.Ok.state).toBe("PullPending");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
  });

  it("does not confirm a submitted transaction while its receipt is missing", async () => {
    const { evm, bridge } = await setup();
    await (evm.actor as any).set_receipt_mode({ Missing: null });
    const args = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n };
    const result: any = await (bridge.actor as any).request_deposit(args);
    const receiptCallsBeforeReplay = await (evm.actor as any).receipt_call_count();
    const replay: any = await (bridge.actor as any).request_deposit(args);
    expect(replay.Ok.settlement).toEqual([]);
    expect(await (evm.actor as any).receipt_call_count()).toBe(receiptCallsBeforeReplay);
    expect(await (bridge.actor as any).continue_deposit(result.Ok.deposit_id)).toHaveProperty("Err.AutomaticProgressPending");
    await advanceAutomaticConfirmation(20);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(stored[0].state).toBe("MintPending");
  });

  it("checks safe confirmation only at 2, 5, and 10 minutes then reports failure", async () => {
    const { evm, bridge } = await setup();
    await (evm.actor as any).set_receipt_mode({ Missing: null });
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
    expect((await (bridge.actor as any).get_deposit(result.Ok.deposit_id))[0].next_automatic_confirmation_check_at_ns).toHaveLength(1);
    expect(await (evm.actor as any).receipt_call_count()).toBe(0n);
    await advanceAutomaticConfirmation(1);
    expect(await (evm.actor as any).receipt_call_count()).toBe(0n);
    for (const minutes of [1, 3, 5]) {
      await advanceAutomaticConfirmation(minutes);
    }
    expect(await (evm.actor as any).receipt_call_count()).toBe(3n);
    const exhausted: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(exhausted[0].last_settlement_stop_reason).toEqual(["Base transaction did not reach the Safe head within 10 minutes"]);
    expect(exhausted[0].next_automatic_confirmation_check_at_ns).toEqual([]);
    expect((await (bridge.actor as any).get_bridge_status()).confirmation_scheduler.scheduled_operations).toBe(0n);
    await advanceAutomaticConfirmation(60);
    expect(await (evm.actor as any).receipt_call_count()).toBe(3n);
  });

  it("restores a pending confirmation timer after upgrade without double execution", async () => {
    const { evm, bridge } = await setup();
    await (evm.actor as any).set_receipt_mode({ Missing: null });
    await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
    await advanceAutomaticConfirmation(1);
    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    await advanceAutomaticConfirmation(1);
    expect(await (evm.actor as any).receipt_call_count()).toBe(1n);
  });

  it("serializes due schedules without letting the earliest missing receipt starve later records", async () => {
    const { evm, bridge } = await setup();
    await (evm.actor as any).set_receipt_mode({ Missing: null });
    const first: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
    const second: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(5), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
    expect((await (bridge.actor as any).get_bridge_status()).confirmation_scheduler.scheduled_operations).toBe(2n);
    await advanceAutomaticConfirmation(20);
    expect(await (evm.actor as any).receipt_call_count()).toBe(2n);
    expect((await (bridge.actor as any).get_deposit(first.Ok.deposit_id))[0].next_automatic_confirmation_check_at_ns).toHaveLength(1);
    expect((await (bridge.actor as any).get_deposit(second.Ok.deposit_id))[0].next_automatic_confirmation_check_at_ns).toHaveLength(1);
  });

  it("stops automatic retry after an RPC confirmation failure", async () => {
    const { evm, bridge } = await setup();
    await (evm.actor as any).set_receipt_mode({ RpcFailure: null });
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
    await advanceAutomaticConfirmation(20);
    const stopped: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(stopped[0].last_settlement_stop_reason).toEqual(["Base RPC unavailable"]);
    expect(stopped[0].next_automatic_confirmation_check_at_ns).toEqual([]);
    expect(await (evm.actor as any).receipt_call_count()).toBe(1n);
    await advanceAutomaticConfirmation(60);
    expect(await (evm.actor as any).receipt_call_count()).toBe(1n);
  });

  it("rate-limits manual settlement retries before the external call", async () => {
    const { ledger, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode({ TemporarilyUnavailable: null });
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
    for (let attempt = 0; attempt < 3; attempt += 1) {
      expect(await (bridge.actor as any).continue_deposit(result.Ok.deposit_id)).toHaveProperty("Ok.Stopped");
    }
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(4n);
    expect(await (bridge.actor as any).continue_deposit(result.Ok.deposit_id)).toHaveProperty("Err.RateLimited");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(4n);
  });

  it("terminalizes a Safe-confirmed EVM revert, pauses deposits, and never rebroadcasts it", async () => {
    const { evm, bridge } = await setup();
    await (evm.actor as any).set_receipt_mode({ Reverted: null });
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
    await advanceAutomaticConfirmation(20);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(stored[0].state).toBe("MintReverted");
    const status: any = await (bridge.actor as any).get_bridge_status();
    expect(status.deposits_paused).toBe(true);
    expect(status.counts.reverted_evm_operations).toBe(1n);
    expect(await (bridge.actor as any).resume_new_deposits()).toEqual({ Err: { UnresolvedEvmRevert: null } });
    const audit: any = await (bridge.actor as any).get_audit_events(0n, 100);
    const reverted = audit.Ok.events.find((event: any) => "EvmOperationReverted" in event.kind);
    expect(reverted.kind.EvmOperationReverted.kind).toEqual({ MintDeposit: null });
    expect(reverted.kind.EvmOperationReverted.transaction_hash).toHaveLength(32);
    expect(reverted.kind.EvmOperationReverted.confirmed_head_block_number).toBeGreaterThan(0n);
    const before = await (evm.actor as any).broadcast_transactions();
    expect(before).toHaveLength(1);
    await advanceTimeWithoutSettlement(4);
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(1);
    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    const reopened: any = await (bridge.actor as any).get_bridge_status();
    expect(reopened.deposits_paused).toBe(true);
    expect(reopened.counts.reverted_evm_operations).toBe(1n);
  });

  it("terminalizes an acknowledgement revert and continues a later Withdrawal settlement", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    await (evm.actor as any).set_receipt_mode({ Confirmed: null });
    const revertedId = new Uint8Array(32).fill(51);
    await (evm.actor as any).set_withdrawal([{ id: revertedId, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    await notifyFixtureWithdrawal(bridge);
    await (evm.actor as any).set_receipt_mode({ Reverted: null });
    await advanceAutomaticConfirmation(20);
    expect((await (bridge.actor as any).get_withdrawal(revertedId))[0].state).toBe("AcknowledgeReverted");
    expect((await (bridge.actor as any).get_bridge_status()).counts.reverted_evm_operations).toBe(1n);
    const audit: any = await (bridge.actor as any).get_audit_events(0n, 100);
    const reverted = audit.Ok.events.find((event: any) => "EvmOperationReverted" in event.kind);
    expect(reverted.kind.EvmOperationReverted.kind).toEqual({ AcknowledgeRelease: null });
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(1);
    await advanceTimeWithoutSettlement(3);
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(1);
    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    expect((await (bridge.actor as any).get_withdrawal(revertedId))[0].state).toBe("AcknowledgeReverted");

    await (evm.actor as any).set_receipt_mode({ Confirmed: null });
    await (evm.actor as any).set_safe_block_sequence(Array(30).fill(200n));
    const nextId = new Uint8Array(32).fill(52);
    await (evm.actor as any).set_withdrawal([{ id: nextId, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    await notifyFixtureWithdrawal(bridge);
    await (evm.actor as any).set_safe_block_sequence([98n]);
    await advanceAutomaticConfirmation(2);
    expect((await (bridge.actor as any).get_withdrawal(nextId))[0].state).toBe("AcknowledgePending");
    await (evm.actor as any).set_safe_block_sequence([200n]);
    await advanceAutomaticConfirmation(3);
    expect((await (bridge.actor as any).get_withdrawal(nextId))[0].state).toBe("Released");
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(2);
  });

  it("terminalizes and preserves a refund revert without rebroadcasting", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    await (evm.actor as any).set_receipt_mode({ Confirmed: null });
    const id = new Uint8Array(32).fill(53);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 2n, min_amount_out: 2n }]);
    await notifyFixtureWithdrawal(bridge);
    await advanceAutomaticConfirmation(20);
    await (evm.actor as any).set_receipt_mode({ Reverted: null });
    await advanceAutomaticConfirmation(20);
    expect((await (bridge.actor as any).get_withdrawal(id))[0].state).toBe("RefundReverted");
    const audit: any = await (bridge.actor as any).get_audit_events(0n, 100);
    const reverted = audit.Ok.events.find((event: any) => "EvmOperationReverted" in event.kind);
    expect(reverted.kind.EvmOperationReverted.kind).toEqual({ RefundWithdrawal: null });
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(2);
    await advanceTimeWithoutSettlement(3);
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(2);
    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    expect((await (bridge.actor as any).get_withdrawal(id))[0].state).toBe("RefundReverted");
    expect((await (bridge.actor as any).get_bridge_status()).counts.reverted_evm_operations).toBe(1n);
  });

  it("pauses only new deposits and allows Governance to resume them", async () => {
    const { bridge, runtimePrincipal } = await setup();
    expect(await (bridge.actor as any).pause_new_deposits()).toHaveProperty("Ok");
    const args = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n };
    expect(await (bridge.actor as any).request_deposit(args)).toEqual({ Err: { DepositsPaused: null } });
    expect(await (bridge.actor as any).resume_new_deposits()).toHaveProperty("Ok");
    expect(await (bridge.actor as any).request_deposit(args)).toHaveProperty("Ok");
    const audit: any = await (bridge.actor as any).get_audit_events(0n, 100);
    expect(audit.Ok.events.length).toBeGreaterThanOrEqual(2);
    expect(await (bridge.actor as any).request_fee_payout(1n)).toEqual({ Err: { InsufficientFeeReserve: null } });
    const second = { ...args, owner_sequence: 1n };
    expect(await (bridge.actor as any).request_deposit(second)).toHaveProperty("Ok");
    const firstPage: any = await (bridge.actor as any).list_deposit_ids({ owner: runtimePrincipal, before_cursor: [], limit: 20 });
    await advanceAutomaticConfirmation(20);
    for (const id of firstPage.Ok.deposit_ids) {
      expect((await (bridge.actor as any).get_deposit(id))[0].state).toBe("Minted");
    }
    expect(await (bridge.actor as any).request_fee_payout(1n)).toHaveProperty("Ok");
  });

  it("installs with new deposits paused until Governance activates them", async () => {
    const { bridge } = await setup(false);
    expect((await (bridge.actor as any).get_bridge_status()).deposits_paused).toBe(true);
    const args = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n };
    expect(await (bridge.actor as any).request_deposit(args)).toEqual({ Err: { DepositsPaused: null } });
  });

  it("rejects a new deposit before ledger pull when Settlement Reserve is insufficient", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_eth_balance(0n);
    const args = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n };
    expect(await (bridge.actor as any).request_deposit(args)).toEqual({ Err: { ReserveUnavailable: null } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);
  });

  it("cancels a definitive Ledger pull failure and releases its Mint reservation", async () => {
    const { ledger, bridge, runtimePrincipal } = await setup();
    const failed = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n };
    await (ledger.actor as any).set_ledger_mode({ BadFee: null });
    expect(await (bridge.actor as any).request_deposit(failed)).toHaveProperty("Ok.settlement.0.Complete");
    let status: any = await (bridge.actor as any).get_bridge_status();
    expect(status.counts.reserved_deposit_mint_amount).toBe(0n);
    const replay: any = await (bridge.actor as any).request_deposit(failed);
    expect(replay.Ok.state).toBe("Cancelled");

    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    await advanceTimeWithoutSettlement(2);
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);
    const replacement = { ...failed, owner_sequence: 1n };
    expect(await (bridge.actor as any).request_deposit(replacement)).toHaveProperty("Ok");
    status = await (bridge.actor as any).get_bridge_status();
    expect(status.counts.reserved_deposit_mint_amount).toBe(99n);
  });

  it("fails a retryable fee payout without trapping its reserve", async () => {
    const { ledger, bridge, runtimePrincipal } = await setup();
    for (const tag of [56, 57]) {
      const deposit: any = await (bridge.actor as any).request_deposit({ owner_sequence: BigInt(tag - 56), base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
      expect(deposit).toHaveProperty("Ok");
    }
    const page: any = await (bridge.actor as any).list_deposit_ids({ owner: runtimePrincipal, before_cursor: [], limit: 20 });
    await advanceAutomaticConfirmation(20);
    for (const id of page.Ok.deposit_ids) {
      expect((await (bridge.actor as any).get_deposit(id))[0].state).toBe("Minted");
    }
    await (ledger.actor as any).set_ledger_mode({ TemporarilyUnavailable: null });
    const failed: any = await (bridge.actor as any).request_fee_payout(1n);
    expect(failed.Ok.state).toEqual({ Pending: null });
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    const retried: any = await (bridge.actor as any).continue_fee_payout(failed.Ok.id);
    expect(retried).toHaveProperty("Ok.Complete");
  });
});
