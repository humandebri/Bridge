import { readFileSync } from "node:fs";
import { spawn, type ChildProcess } from "node:child_process";
import { createServer } from "node:net";
import { resolve } from "node:path";
import { IDL } from "@icp-sdk/core/candid";
import { Principal } from "@icp-sdk/core/principal";
import { PocketIc, SubnetStateType } from "@dfinity/pic";

const root = resolve(__dirname, "..");
const bridgeWasm = resolve(root, "target/wasm32-unknown-unknown/release/bridge_canister.wasm");
const mockWasm = resolve(root, "target/wasm32-unknown-unknown/release/mock_external.wasm");
const watchdogWasm = resolve(root, "target/wasm32-unknown-unknown/release/pause_watchdog.wasm");

const mockInit = IDL.Record({ ledger_id: IDL.Principal });
const ledgerMode = IDL.Variant({ Succeed: IDL.Null, Duplicate: IDL.Null, Trap: IDL.Null, BadFee: IDL.Null, TemporarilyUnavailable: IDL.Null });
const receiptMode = IDL.Variant({ Finalized: IDL.Null, Missing: IDL.Null, Reverted: IDL.Null });
const withdrawalFixture = IDL.Record({ id: IDL.Vec(IDL.Nat8), owner: IDL.Vec(IDL.Nat8), subaccount: IDL.Vec(IDL.Nat8), amount: IDL.Nat, min_amount_out: IDL.Nat });
const chainKeyProbe = IDL.Record({ public_key: IDL.Vec(IDL.Nat8), signature: IDL.Vec(IDL.Nat8) });
const mockIdl = ({ IDL: I }: { IDL: typeof IDL }) =>
  I.Service({
    set_ledger_mode: I.Func([ledgerMode], [], []),
    set_withdrawal: I.Func([I.Opt(withdrawalFixture)], [], []),
    set_receipt_mode: I.Func([receiptMode], [], []),
    set_eth_balance: I.Func([I.Nat], [], []),
    set_next_evm_nonce: I.Func([I.Nat64], [], []),
    set_service_fee: I.Func([I.Nat], [], []),
    set_mint_window: I.Func([I.Nat, I.Nat, I.Nat64, I.Nat64, I.Nat64], [], []),
    set_finalized_block_sequence: I.Func([I.Vec(I.Nat64)], [], []),
    set_safe_block_sequence: I.Func([I.Vec(I.Nat64)], [], []),
    broadcast_transactions: I.Func([], [I.Vec(I.Vec(I.Nat8))], ["query"]),
    ledger_transactions: I.Func([], [I.Vec(I.Record({ kind: I.Text, mint: I.Opt(I.Reserved), burn: I.Opt(I.Reserved), transfer: I.Opt(I.Reserved), approve: I.Opt(I.Reserved), fee_collector: I.Opt(I.Reserved), timestamp: I.Nat64 }))], ["query"]),
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
  poll_interval_seconds: IDL.Nat64,
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
  client_request_id: IDL.Vec(IDL.Nat8),
  base_recipient: IDL.Vec(IDL.Nat8),
  gross_amount: IDL.Nat,
  max_service_fee: IDL.Nat,
});
const depositReceipt = IDL.Record({ deposit_id: IDL.Vec(IDL.Nat8), state: IDL.Text });
const depositError = IDL.Variant({
  BaseObservationUnavailable: IDL.Null,
  Rejected: IDL.Text,
  InvalidRequest: IDL.Text,
  LedgerFeeUnavailable: IDL.Null,
  StorageFailure: IDL.Null,
  DepositsPaused: IDL.Null,
  ReserveUnavailable: IDL.Null,
});
const baseConfirmation = IDL.Variant({
  Submitted: IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8) }),
  SafeSucceeded: IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8), receipt_block_number: IDL.Nat64, observed_head: IDL.Nat64 }),
  SafeReverted: IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8), receipt_block_number: IDL.Nat64, observed_head: IDL.Nat64 }),
  Finalized: IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8), receipt_block_number: IDL.Nat64, observed_head: IDL.Nat64 }),
  Reverted: IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8), receipt_block_number: IDL.Nat64, observed_head: IDL.Nat64 }),
});
const depositView = IDL.Record({
  deposit_id: IDL.Vec(IDL.Nat8),
  gross_amount: IDL.Nat,
  net_amount: IDL.Nat,
  service_fee: IDL.Nat,
  base_recipient: IDL.Vec(IDL.Nat8),
  state: IDL.Text,
  base_confirmation: IDL.Opt(baseConfirmation),
});
const withdrawalView = IDL.Record({ withdrawal_id: IDL.Vec(IDL.Nat8), amount: IDL.Nat, min_amount_out: IDL.Nat, state: IDL.Text, base_confirmation: IDL.Opt(baseConfirmation) });
const reserveStatus = IDL.Record({ eth_balance_wei: IDL.Nat, cycles_balance: IDL.Nat, required_eth_wei: IDL.Nat, required_cycles: IDL.Nat, eth_surplus_wei: IDL.Nat, cycles_surplus: IDL.Nat, sufficient: IDL.Bool });
const bridgeStatus = IDL.Record({ schema_version: IDL.Nat16, last_finalized_base_block: IDL.Nat64, last_safe_base_block: IDL.Nat64, last_reserve_observation_ns: IDL.Nat64, last_finalized_observation_ns: IDL.Nat64, last_safe_observation_ns: IDL.Nat64, withdrawal_log_cursor: IDL.Nat64, counts: IDL.Record({ deposits: IDL.Nat64, withdrawals: IDL.Nat64, pending_ledger_operations: IDL.Nat64, pending_evm_operations: IDL.Nat64, reconciliation_holds: IDL.Nat64, reserved_deposit_mint_amount: IDL.Nat, reverted_evm_operations: IDL.Nat64 }), reserve: reserveStatus, deposits_paused: IDL.Bool, queued_evm_operations: IDL.Nat64, safe_evm_operations: IDL.Nat64, last_audit_sequence: IDL.Opt(IDL.Nat64) });
const adminError = IDL.Variant({ Unauthorized: IDL.Null, InvalidArgument: IDL.Text, StorageFailure: IDL.Null, InsufficientFeeReserve: IDL.Null, UnresolvedEvmRevert: IDL.Null });
const auditedEvmOperationKind = IDL.Variant({ RefundWithdrawal: IDL.Null, MintDeposit: IDL.Null, AcknowledgeRelease: IDL.Null });
const auditEventKind = IDL.Variant({
  RuntimeAdministratorsRotated: IDL.Null,
  BaseServiceFeeChanged: IDL.Reserved,
  EvmOperationReverted: IDL.Record({ finalized_block_number: IDL.Nat64, transaction_hash: IDL.Vec(IDL.Nat8), kind: auditedEvmOperationKind, operation_id: IDL.Nat64 }),
  DepositsPauseRepeated: IDL.Null,
  FeeRecipientChanged: IDL.Reserved,
  DepositsPaused: IDL.Null,
  DepositsResumed: IDL.Null,
  FeePayoutRequested: IDL.Reserved,
  ReserveGateChanged: IDL.Reserved,
});
const auditEvent = IDL.Record({ timestamp_ns: IDL.Nat64, kind: auditEventKind, caller: IDL.Principal, sequence: IDL.Nat64 });
const payoutState = IDL.Variant({ Pending: IDL.Null, ReconciliationHold: IDL.Null, Succeeded: IDL.Record({ block_index: IDL.Nat }), Failed: IDL.Null });
const payoutReceipt = IDL.Record({ id: IDL.Nat64, amount: IDL.Nat, state: payoutState });
const bridgeIdl = ({ IDL: I }: { IDL: typeof IDL }) =>
  I.Service({
    request_deposit: I.Func(
      [depositArgs],
      [I.Variant({ Ok: depositReceipt, Err: depositError })],
      [],
    ),
    get_deposit: I.Func([I.Vec(I.Nat8)], [I.Opt(depositView)], ["query"]),
    get_withdrawal: I.Func([I.Vec(I.Nat8)], [I.Opt(withdrawalView)], ["query"]),
    get_bridge_status: I.Func([], [bridgeStatus], ["query"]),
    pause_new_deposits: I.Func([], [I.Variant({ Ok: I.Null, Err: adminError })], []),
    resume_new_deposits: I.Func([], [I.Variant({ Ok: I.Null, Err: adminError })], []),
    get_audit_events: I.Func([I.Nat64, I.Nat16], [I.Variant({ Ok: I.Vec(auditEvent), Err: adminError })], ["query"]),
    request_fee_payout: I.Func([I.Nat], [I.Variant({ Ok: payoutReceipt, Err: adminError })], []),
  });
const watchdogInit = IDL.Record({ bridge_canister: IDL.Principal, poll_interval_seconds: IDL.Nat64, stale_after_seconds: IDL.Nat64, failure_threshold: IDL.Nat8 });
const watchdogStatus = IDL.Record({ consecutive_failures: IDL.Nat8, last_success_ns: IDL.Nat64, last_pause_attempt_ns: IDL.Nat64, pause_attempts: IDL.Nat64 });
const watchdogIdl = ({ IDL: I }: { IDL: typeof IDL }) => I.Service({ get_watchdog_status: I.Func([], [watchdogStatus], ["query"]) });

describe("Phase 3 PocketIC saga", () => {
  let server: ChildProcess | undefined;
  let pic: PocketIc | undefined;
  let serverUrl = "";

  async function setup(watchdogCanisterId?: Principal) {
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
    const init = { ledger_canister_id: ledger.canisterId, index_canister_id: index.canisterId, evm_rpc_canister_id: evm.canisterId, custom_evm_rpc_urls: [], base_chain_id: 8453n, bridge_contract: new Uint8Array(20).fill(1), ecdsa_key_name: "key_1", ecdsa_derivation_path: [], poll_interval_seconds: 60n, transaction_gas_limit: 500_000n, max_fee_per_gas: 10n, max_priority_fee_per_gas: 1n, eth_floor_wei: 1n, cycles_floor: 1n, settlement_cycle_ceiling: 1n, governance_principal: runtimePrincipal, pause_principals: watchdogCanisterId === undefined ? [runtimePrincipal] : [runtimePrincipal, watchdogCanisterId], finance_administrator: runtimePrincipal, fee_recipient: { owner: runtimePrincipal, subaccount: [] } };
    const bridge = await pic!.setupCanister({ idlFactory: bridgeIdl, wasm: readFileSync(bridgeWasm), arg: IDL.encode([bridgeInit], [init]), cycles: 500_000_000_000_000n, targetSubnetId: subnet.id });
    bridge.actor.setPrincipal(runtimePrincipal);
    expect((await pic!.getCanisterSubnetId(bridge.canisterId))?.toText()).toBe(subnet.id.toText());
    return { ledger, index, evm, bridge, init };
  }

  async function runTimers(rounds = 5) { for (let step = 0; step < rounds; step += 1) { await pic!.advanceTime(60_000); await pic!.tick(5); } }

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
    await new Promise((resolveReady) => setTimeout(resolveReady, 500));
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

  it("persists one idempotent Deposit through ledger pull, EVM submission, and finalized mint", async () => {
    const { bridge } = await setup();

    const request = {
      client_request_id: new Uint8Array(32).fill(3),
      base_recipient: new Uint8Array(20).fill(4),
      gross_amount: 100n,
      max_service_fee: 10n,
    };
    const first: any = await (bridge.actor as any).request_deposit(request);
    if (!("Ok" in first)) {
      throw new Error(`request_deposit failed: ${JSON.stringify(first)}`);
    }
    expect(first.Ok.state).toBe("MintPending");
    const replay: any = await (bridge.actor as any).request_deposit(request);
    expect(Array.from(replay.Ok.deposit_id)).toEqual(Array.from(first.Ok.deposit_id));

    await runTimers(4);
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

  it("freezes the accepted service fee across a later Base fee change", async () => {
    const { evm, bridge } = await setup();
    const result: any = await (bridge.actor as any).request_deposit({ client_request_id: new Uint8Array(32).fill(41), base_recipient: new Uint8Array(20).fill(4), gross_amount: 100n, max_service_fee: 10n });
    expect(result).toHaveProperty("Ok");
    await (evm.actor as any).set_service_fee(7n);
    await runTimers(4);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(stored[0].service_fee).toBe(1n);
    expect(stored[0].net_amount).toBe(99n);
    expect(stored[0].state).toBe("Minted");
  });

  it("reserves pending Mint capacity and rejects overflow before ledger pull", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_mint_window(90n, 100n, 0n, 100n, 1n);
    const first: any = await (bridge.actor as any).request_deposit({ client_request_id: new Uint8Array(32).fill(42), base_recipient: new Uint8Array(20).fill(4), gross_amount: 10n, max_service_fee: 10n });
    expect(first).toHaveProperty("Ok");
    const second: any = await (bridge.actor as any).request_deposit({ client_request_id: new Uint8Array(32).fill(43), base_recipient: new Uint8Array(20).fill(4), gross_amount: 3n, max_service_fee: 10n });
    expect(second).toHaveProperty("Err.Rejected");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
    const status: any = await (bridge.actor as any).get_bridge_status();
    expect(status.counts.reserved_deposit_mint_amount).toBe(9n);
  });

  it("treats a full expired Mint window as having zero effective consumption", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_mint_window(100n, 100n, 0n, 10n, 10n);
    const accepted: any = await (bridge.actor as any).request_deposit({ client_request_id: new Uint8Array(32).fill(44), base_recipient: new Uint8Array(20).fill(4), gross_amount: 10n, max_service_fee: 10n });
    expect(accepted).toHaveProperty("Ok");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("reobserves stale Mint snapshots and fails closed after three stale blocks", async () => {
    const { ledger, evm, bridge } = await setup();
    const seed: any = await (bridge.actor as any).request_deposit({ client_request_id: new Uint8Array(32).fill(47), base_recipient: new Uint8Array(20).fill(4), gross_amount: 10n, max_service_fee: 10n });
    await runTimers(4);
    expect((await (bridge.actor as any).get_deposit(seed.Ok.deposit_id))[0].state).toBe("Minted");

    await (evm.actor as any).set_finalized_block_sequence([98n, 100n]);
    const refreshed: any = await (bridge.actor as any).request_deposit({ client_request_id: new Uint8Array(32).fill(48), base_recipient: new Uint8Array(20).fill(4), gross_amount: 10n, max_service_fee: 10n });
    expect(refreshed).toHaveProperty("Ok");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(2);

    await (evm.actor as any).set_finalized_block_sequence([98n, 98n, 98n]);
    const unavailable: any = await (bridge.actor as any).request_deposit({ client_request_id: new Uint8Array(32).fill(49), base_recipient: new Uint8Array(20).fill(4), gross_amount: 10n, max_service_fee: 10n });
    expect(unavailable).toEqual({ Err: { BaseObservationUnavailable: null } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(2);
  });

  it("rechecks pause atomically after request awaits while preserving idempotent retries", async () => {
    const { ledger, bridge } = await setup();
    const args = { client_request_id: new Uint8Array(32).fill(50), base_recipient: new Uint8Array(20).fill(4), gross_amount: 100n, max_service_fee: 10n };
    const pending = (bridge.actor as any).request_deposit(args);
    await pic!.tick(1);
    await (bridge.actor as any).pause_new_deposits();
    expect(await pending).toEqual({ Err: { DepositsPaused: null } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);

    await (bridge.actor as any).resume_new_deposits();
    const accepted: any = await (bridge.actor as any).request_deposit(args);
    expect(accepted).toHaveProperty("Ok");
    await (bridge.actor as any).pause_new_deposits();
    const replay: any = await (bridge.actor as any).request_deposit(args);
    expect(Array.from(replay.Ok.deposit_id)).toEqual(Array.from(accepted.Ok.deposit_id));
  });

  it("discovers a finalized burn, releases ICP, and finalizes acknowledgement", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_next_evm_nonce(7n);
    const id = new Uint8Array(32).fill(6);
    await (evm.actor as any).set_withdrawal([{ id, owner: Principal.selfAuthenticating(new Uint8Array(32).fill(8)).toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    await runTimers(7);
    const withdrawal: any = await (bridge.actor as any).get_withdrawal(id);
    expect(withdrawal[0].state).toBe("Released");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
    const broadcasts = await (evm.actor as any).broadcast_transactions();
    expect(broadcasts.length).toBeGreaterThanOrEqual(1);
    expect(broadcasts.every((raw: Uint8Array) => Buffer.from(raw).equals(Buffer.from(broadcasts[0])))).toBe(true);
  });

  it("continues an ambiguous Withdrawal release from reconciled Hold to acknowledgement", async () => {
    const { ledger, evm, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    const id = new Uint8Array(32).fill(46);
    await (evm.actor as any).set_withdrawal([{ id, owner: Principal.selfAuthenticating(new Uint8Array(32).fill(8)).toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    await runTimers(3);
    const held: any = await (bridge.actor as any).get_withdrawal(id);
    expect(held[0].state).toBe("ReconciliationHold");
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    await runTimers(7);
    const released: any = await (bridge.actor as any).get_withdrawal(id);
    expect(released[0].state).toBe("Released");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("refunds an uneconomic burn without sending ICP", async () => {
    const { ledger, evm, bridge } = await setup();
    const id = new Uint8Array(32).fill(9);
    await (evm.actor as any).set_withdrawal([{ id, owner: Principal.selfAuthenticating(new Uint8Array(32).fill(8)).toUint8Array(), subaccount: new Uint8Array(32), amount: 2n, min_amount_out: 2n }]);
    await runTimers(7);
    const withdrawal: any = await (bridge.actor as any).get_withdrawal(id);
    expect(withdrawal[0].state).toBe("Refunded");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);
    const broadcasts = await (evm.actor as any).broadcast_transactions();
    expect(broadcasts.length).toBeGreaterThanOrEqual(1);
    expect(broadcasts.every((raw: Uint8Array) => Buffer.from(raw).equals(Buffer.from(broadcasts[0])))).toBe(true);
  });

  it("continues an ambiguous deposit from reconciled Hold to Mint", async () => {
    const { ledger, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    const result: any = await (bridge.actor as any).request_deposit({ client_request_id: new Uint8Array(32).fill(11), base_recipient: new Uint8Array(20).fill(4), gross_amount: 100n, max_service_fee: 10n });
    expect(result.Ok.state).toBe("ReconciliationHold");
    const before: any = await (bridge.actor as any).get_bridge_status();
    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    const after: any = await (bridge.actor as any).get_bridge_status();
    expect(after.counts.reconciliation_holds).toBe(before.counts.reconciliation_holds);
    expect(after.counts.reconciliation_holds).toBe(1n);
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    await runTimers(6);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(stored[0].state).toBe("Minted");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("does not finalize a submitted transaction while its receipt is missing", async () => {
    const { evm, bridge } = await setup();
    await (evm.actor as any).set_receipt_mode({ Missing: null });
    const result: any = await (bridge.actor as any).request_deposit({ client_request_id: new Uint8Array(32).fill(12), base_recipient: new Uint8Array(20).fill(4), gross_amount: 100n, max_service_fee: 10n });
    await runTimers(5);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(stored[0].state).toBe("MintPending");
  });

  it("persists safe progress without terminalizing, clears it on regression, and finalizes later", async () => {
    const { evm, bridge } = await setup();
    const result: any = await (bridge.actor as any).request_deposit({ client_request_id: new Uint8Array(32).fill(54), base_recipient: new Uint8Array(20).fill(4), gross_amount: 100n, max_service_fee: 10n });
    await (evm.actor as any).set_safe_block_sequence(Array(20).fill(100n));
    await (evm.actor as any).set_finalized_block_sequence(Array(20).fill(98n));
    await runTimers(3);

    const safe: any = (await (bridge.actor as any).get_deposit(result.Ok.deposit_id))[0];
    expect(safe.state).toBe("MintPending");
    expect(safe.base_confirmation[0]).toHaveProperty("SafeSucceeded");
    expect(safe.base_confirmation[0].SafeSucceeded.receipt_block_number).toBe(99n);
    expect((await (bridge.actor as any).get_bridge_status()).safe_evm_operations).toBe(1n);
    const rawBeforeRegression = await (evm.actor as any).broadcast_transactions();

    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    expect(((await (bridge.actor as any).get_deposit(result.Ok.deposit_id))[0].base_confirmation[0])).toHaveProperty("SafeSucceeded");

    await (evm.actor as any).set_safe_block_sequence(Array(10).fill(98n));
    await runTimers(1);
    const regressed: any = (await (bridge.actor as any).get_deposit(result.Ok.deposit_id))[0];
    expect(regressed.state).toBe("MintPending");
    expect(regressed.base_confirmation[0]).toHaveProperty("Submitted");
    expect((await (bridge.actor as any).get_bridge_status()).safe_evm_operations).toBe(0n);
    const rawAfterRegression = await (evm.actor as any).broadcast_transactions();
    expect(rawAfterRegression.length).toBeGreaterThan(rawBeforeRegression.length);
    expect(rawAfterRegression.every((raw: Uint8Array) => Buffer.from(raw).equals(Buffer.from(rawBeforeRegression[0])))).toBe(true);

    await (evm.actor as any).set_safe_block_sequence(Array(10).fill(100n));
    await (evm.actor as any).set_finalized_block_sequence(Array(10).fill(100n));
    await runTimers(2);
    const finalized: any = (await (bridge.actor as any).get_deposit(result.Ok.deposit_id))[0];
    expect(finalized.state).toBe("Minted");
    expect(finalized.base_confirmation[0]).toHaveProperty("Finalized");
    expect((await (bridge.actor as any).get_bridge_status()).safe_evm_operations).toBe(0n);
  });

  it("terminalizes a finalized EVM revert, pauses deposits, and never rebroadcasts it", async () => {
    const { evm, bridge } = await setup();
    await (evm.actor as any).set_receipt_mode({ Reverted: null });
    const result: any = await (bridge.actor as any).request_deposit({ client_request_id: new Uint8Array(32).fill(45), base_recipient: new Uint8Array(20).fill(4), gross_amount: 100n, max_service_fee: 10n });
    await runTimers(5);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(stored[0].state).toBe("MintReverted");
    const status: any = await (bridge.actor as any).get_bridge_status();
    expect(status.deposits_paused).toBe(true);
    expect(status.counts.reverted_evm_operations).toBe(1n);
    expect(await (bridge.actor as any).resume_new_deposits()).toEqual({ Err: { UnresolvedEvmRevert: null } });
    const audit: any = await (bridge.actor as any).get_audit_events(0n, 100);
    const reverted = audit.Ok.find((event: any) => "EvmOperationReverted" in event.kind);
    expect(reverted.kind.EvmOperationReverted.kind).toEqual({ MintDeposit: null });
    expect(reverted.kind.EvmOperationReverted.transaction_hash).toHaveLength(32);
    expect(reverted.kind.EvmOperationReverted.finalized_block_number).toBeGreaterThan(0n);
    const before = await (evm.actor as any).broadcast_transactions();
    expect(before).toHaveLength(1);
    await runTimers(4);
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(1);
    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    const reopened: any = await (bridge.actor as any).get_bridge_status();
    expect(reopened.deposits_paused).toBe(true);
    expect(reopened.counts.reverted_evm_operations).toBe(1n);
  });

  it("terminalizes an acknowledgement revert and continues a later Withdrawal settlement", async () => {
    const { evm, bridge } = await setup();
    await (evm.actor as any).set_receipt_mode({ Reverted: null });
    const revertedId = new Uint8Array(32).fill(51);
    await (evm.actor as any).set_withdrawal([{ id: revertedId, owner: Principal.selfAuthenticating(new Uint8Array(32).fill(8)).toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    await runTimers(7);
    expect((await (bridge.actor as any).get_withdrawal(revertedId))[0].state).toBe("AcknowledgeReverted");
    expect((await (bridge.actor as any).get_bridge_status()).counts.reverted_evm_operations).toBe(1n);
    const audit: any = await (bridge.actor as any).get_audit_events(0n, 100);
    const reverted = audit.Ok.find((event: any) => "EvmOperationReverted" in event.kind);
    expect(reverted.kind.EvmOperationReverted.kind).toEqual({ AcknowledgeRelease: null });
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(1);
    await runTimers(3);
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(1);
    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    expect((await (bridge.actor as any).get_withdrawal(revertedId))[0].state).toBe("AcknowledgeReverted");

    await (evm.actor as any).set_receipt_mode({ Finalized: null });
    await (evm.actor as any).set_finalized_block_sequence(Array(30).fill(200n));
    const nextId = new Uint8Array(32).fill(52);
    await (evm.actor as any).set_withdrawal([{ id: nextId, owner: Principal.selfAuthenticating(new Uint8Array(32).fill(8)).toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, min_amount_out: 90n }]);
    await runTimers(7);
    expect((await (bridge.actor as any).get_withdrawal(nextId))[0].state).toBe("Released");
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(2);
  });

  it("terminalizes and preserves a refund revert without rebroadcasting", async () => {
    const { evm, bridge } = await setup();
    await (evm.actor as any).set_receipt_mode({ Reverted: null });
    const id = new Uint8Array(32).fill(53);
    await (evm.actor as any).set_withdrawal([{ id, owner: Principal.selfAuthenticating(new Uint8Array(32).fill(8)).toUint8Array(), subaccount: new Uint8Array(32), amount: 2n, min_amount_out: 2n }]);
    await runTimers(7);
    expect((await (bridge.actor as any).get_withdrawal(id))[0].state).toBe("RefundReverted");
    const audit: any = await (bridge.actor as any).get_audit_events(0n, 100);
    const reverted = audit.Ok.find((event: any) => "EvmOperationReverted" in event.kind);
    expect(reverted.kind.EvmOperationReverted.kind).toEqual({ RefundWithdrawal: null });
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(1);
    await runTimers(3);
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(1);
    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    expect((await (bridge.actor as any).get_withdrawal(id))[0].state).toBe("RefundReverted");
    expect((await (bridge.actor as any).get_bridge_status()).counts.reverted_evm_operations).toBe(1n);
  });

  it("pauses through the watchdog on reserve failure and retains watchdog state after upgrade", async () => {
    const subnet = await pic!.getFiduciarySubnet();
    if (subnet === undefined) throw new Error("Fiduciary subnet was not created");
    const watchdogCanisterId = await pic!.createCanister({ cycles: 50_000_000_000_000n, targetSubnetId: subnet.id });
    const { evm, bridge } = await setup(watchdogCanisterId);
    const init = { bridge_canister: bridge.canisterId, poll_interval_seconds: 60n, stale_after_seconds: 900n, failure_threshold: 3 };
    await pic!.installCode({ canisterId: watchdogCanisterId, wasm: readFileSync(watchdogWasm), arg: IDL.encode([watchdogInit], [init]), targetSubnetId: subnet.id });
    const watchdog = pic!.createActor(watchdogIdl, watchdogCanisterId);
    await (evm.actor as any).set_eth_balance(0n);
    const rejected: any = await (bridge.actor as any).request_deposit({ client_request_id: new Uint8Array(32).fill(31), base_recipient: new Uint8Array(20).fill(4), gross_amount: 100n, max_service_fee: 10n });
    expect(rejected).toHaveProperty("Err.ReserveUnavailable");
    await runTimers(2);
    expect((await (bridge.actor as any).get_bridge_status()).deposits_paused).toBe(true);
    const before: any = await (watchdog as any).get_watchdog_status();
    expect(before.pause_attempts).toBeGreaterThanOrEqual(1n);
    await pic!.upgradeCanister({ canisterId: watchdogCanisterId, wasm: readFileSync(watchdogWasm), arg: IDL.encode([], []) });
    const after: any = await (watchdog as any).get_watchdog_status();
    expect(after.pause_attempts).toBe(before.pause_attempts);
  });

  it("pauses only new deposits and allows Governance to resume them", async () => {
    const { bridge } = await setup();
    expect(await (bridge.actor as any).pause_new_deposits()).toHaveProperty("Ok");
    const args = { client_request_id: new Uint8Array(32).fill(21), base_recipient: new Uint8Array(20).fill(4), gross_amount: 100n, max_service_fee: 10n };
    expect(await (bridge.actor as any).request_deposit(args)).toEqual({ Err: { DepositsPaused: null } });
    expect(await (bridge.actor as any).resume_new_deposits()).toHaveProperty("Ok");
    expect(await (bridge.actor as any).request_deposit(args)).toHaveProperty("Ok");
    const audit: any = await (bridge.actor as any).get_audit_events(0n, 100);
    expect(audit.Ok.length).toBeGreaterThanOrEqual(2);
    expect(await (bridge.actor as any).request_fee_payout(1n)).toEqual({ Err: { InsufficientFeeReserve: null } });
    const second = { ...args, client_request_id: new Uint8Array(32).fill(23) };
    expect(await (bridge.actor as any).request_deposit(second)).toHaveProperty("Ok");
    await runTimers(4);
    expect(await (bridge.actor as any).request_fee_payout(1n)).toHaveProperty("Ok");
  });

  it("rejects a new deposit before ledger pull when Settlement Reserve is insufficient", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_eth_balance(0n);
    const args = { client_request_id: new Uint8Array(32).fill(22), base_recipient: new Uint8Array(20).fill(4), gross_amount: 100n, max_service_fee: 10n };
    expect(await (bridge.actor as any).request_deposit(args)).toEqual({ Err: { ReserveUnavailable: null } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);
  });

  it("cancels a definitive Ledger pull failure and releases its Mint reservation", async () => {
    const { ledger, bridge } = await setup();
    const failed = { client_request_id: new Uint8Array(32).fill(54), base_recipient: new Uint8Array(20).fill(4), gross_amount: 100n, max_service_fee: 10n };
    await (ledger.actor as any).set_ledger_mode({ BadFee: null });
    expect(await (bridge.actor as any).request_deposit(failed)).toHaveProperty("Err.Rejected");
    let status: any = await (bridge.actor as any).get_bridge_status();
    expect(status.counts.reserved_deposit_mint_amount).toBe(0n);
    const replay: any = await (bridge.actor as any).request_deposit(failed);
    expect(replay.Ok.state).toBe("Cancelled");

    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    await runTimers(2);
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);
    const replacement = { ...failed, client_request_id: new Uint8Array(32).fill(55) };
    expect(await (bridge.actor as any).request_deposit(replacement)).toHaveProperty("Ok");
    status = await (bridge.actor as any).get_bridge_status();
    expect(status.counts.reserved_deposit_mint_amount).toBe(99n);
  });

  it("fails a retryable fee payout without trapping its reserve", async () => {
    const { ledger, bridge } = await setup();
    for (const tag of [56, 57]) {
      const deposit: any = await (bridge.actor as any).request_deposit({ client_request_id: new Uint8Array(32).fill(tag), base_recipient: new Uint8Array(20).fill(4), gross_amount: 100n, max_service_fee: 10n });
      expect(deposit).toHaveProperty("Ok");
    }
    await runTimers(6);
    await (ledger.actor as any).set_ledger_mode({ TemporarilyUnavailable: null });
    const failed: any = await (bridge.actor as any).request_fee_payout(1n);
    expect(failed.Ok.state).toEqual({ Failed: null });
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    const retried: any = await (bridge.actor as any).request_fee_payout(1n);
    expect(retried.Ok.state).toHaveProperty("Succeeded");
  });
});
