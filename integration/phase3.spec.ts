import { readFileSync } from "node:fs";
import { spawn, type ChildProcess } from "node:child_process";
import { createHash } from "node:crypto";
import { createServer } from "node:net";
import { resolve } from "node:path";
import { IDL } from "@icp-sdk/core/candid";
import { Principal } from "@icp-sdk/core/principal";
import { idlFactory as bridgeIdl, init as bridgeInitFactory } from "./generated/bridge.idl";
import { idlFactory as mockIdl, init as mockInitFactory } from "./generated/mock-external.idl";
import { PocketIc, SubnetStateType } from "@dfinity/pic";

const root = resolve(__dirname, "..");
const bridgeWasm = resolve(root, "target/test-deployment/wasm32-unknown-unknown/release/bridge_canister.wasm");
const mockWasm = resolve(root, "target/wasm32-unknown-unknown/release/mock_external.wasm");

const mockInit = mockInitFactory({ IDL })[0];
const bridgeInit = bridgeInitFactory({ IDL })[0];
const bridgeService: any = bridgeIdl({ IDL });
const depositArgs = bridgeService._fields.find((field: [string, any]) => field[0] === "request_deposit")[1].argTypes[0];
function phaseName(value: Record<string, unknown>): string {
  const keys = Object.keys(value);
  if (keys.length !== 1) throw new Error(`Invalid phase variant: ${JSON.stringify(value)}`);
  return keys[0];
}
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
    const init = { ledger_canister_id: ledger.canisterId, index_canister_id: index.canisterId, evm_rpc_canister_id: evm.canisterId, custom_evm_rpc_urls: [], base_chain_id: 8453n, bridge_contract: new Uint8Array(20).fill(1), timelock_contract: new Uint8Array(20).fill(2), ecdsa_key_name: "key_1", ecdsa_derivation_path: [], governance_ecdsa_derivation_path: [new TextEncoder().encode("governance-operator")], deposit_rate_limit_window_seconds: 60n, deposit_rate_limit_global: 30, deposit_rate_limit_per_principal: 3, settlement_rate_limit_window_seconds: 600n, settlement_rate_limit_global: 60, settlement_rate_limit_per_principal: 6, settlement_rate_limit_per_record: 3, transaction_gas_limit: 500_000n, max_fee_per_gas: 10n, max_priority_fee_per_gas: 1n, evm_liveness: { check_interval_seconds: 60n, rebroadcast_after_seconds: 300n, replacement_after_seconds: 1_800n, max_replacements: 3, fee_bump_bps: 1_250, fee_ceiling_multiplier_bps: 40_000 }, eth_floor_wei: 1n, cycles_floor: 1n, settlement_cycle_ceiling: 1n, governance_principal: runtimePrincipal, pause_principal: Principal.selfAuthenticating(new Uint8Array(32).fill(34)), fee_recipient: { owner: runtimePrincipal, subaccount: [] } };
    const bridge = await pic!.setupCanister({ idlFactory: bridgeIdl, wasm: readFileSync(bridgeWasm), arg: IDL.encode([bridgeInit], [init]), cycles: 500_000_000_000_000n, targetSubnetId: subnet.id });
    bridge.actor.setPrincipal(runtimePrincipal);
    const configuredSigner: any = await (evm.actor as any).set_bridge_signer_for_canister(bridge.canisterId, init.ecdsa_key_name);
    if (!("Ok" in configuredSigner)) throw new Error(`failed to configure mock bridge signer: ${configuredSigner.Err}`);
    // The canister rejects an empty eth_getCode result before admitting deposits.
    // A fixed non-empty mock runtime keeps the observed bridge identity deterministic.
    await (evm.actor as any).set_bridge_runtime_code(new Uint8Array([0x60, 0x00]));
    if (activate) expect(await (bridge.actor as any).resume_new_deposits()).toHaveProperty("Ok");
    expect((await pic!.getCanisterSubnetId(bridge.canisterId))?.toText()).toBe(subnet.id.toText());
    return { ledger, index, evm, bridge, init, runtimePrincipal };
  }

  async function advanceTimeWithoutSettlement(rounds = 5) { for (let step = 0; step < rounds; step += 1) { await pic!.advanceTime(60_000); await pic!.tick(5); } }
  async function advanceClock(minutes: number) {
    await pic!.advanceTime(minutes * 60_000);
    await pic!.tick(30);
  }
  async function confirmDeposit(bridge: any, depositId: Uint8Array, observedFinalizedBlockNumber = 100n, receiptBlockNumber = 99n) {
    const stored: any = await bridge.actor.get_deposit(depositId);
    const submitted = stored[0]?.base_confirmation?.[0]?.Submitted;
    if (submitted === undefined) throw new Error("deposit has no submitted Base transaction");
    return bridge.actor.confirm_deposit({
      settlement_id: depositId,
      transaction_hash: submitted.transaction_hash,
      receipt_block_number: receiptBlockNumber,
      observed_finalized_block_number: observedFinalizedBlockNumber,
    });
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
    server = spawn(resolve("node_modules/@dfinity/pic/pocket-ic"), ["--port", String(port), "--hard-ttl", "1800"], { stdio: "inherit" });
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

  it("persists one idempotent Deposit through ledger pull, EVM submission, and finalized mint", async () => {
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
    expect(phaseName(first.Ok.state)).toBe("MintPending");
    expect(await confirmDeposit(bridge, first.Ok.deposit_id)).toHaveProperty("Ok.Complete");
    const replay: any = await (bridge.actor as any).request_deposit(request);
    expect(Array.from(replay.Ok.deposit_id)).toEqual(Array.from(first.Ok.deposit_id));

    const stored: any = await (bridge.actor as any).get_deposit(first.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("Minted");
    for (let upgrade = 0; upgrade < 2; upgrade += 1) {
      await pic!.upgradeCanister({
        canisterId: bridge.canisterId,
        wasm: readFileSync(bridgeWasm),
        arg: IDL.encode([], []),
      });
      const reopened: any = await (bridge.actor as any).get_deposit(first.Ok.deposit_id);
      expect(phaseName(reopened[0].state)).toBe("Minted");
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

  it("authorizes owners and settlement administrators but rejects anonymous and third-party confirmation calls", async () => {
    const { bridge, runtimePrincipal } = await setup();
    const owner = Principal.selfAuthenticating(new Uint8Array(32).fill(31));
    const thirdParty = Principal.selfAuthenticating(new Uint8Array(32).fill(32));
    const pausePrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(33));
    const deposit = async (tag: number) => (bridge.actor as any).request_deposit({ owner_sequence: BigInt(tag - 91), base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });

    bridge.actor.setPrincipal(owner);
    const ownerDeposit: any = await deposit(91);
    expect(await confirmDeposit(bridge, ownerDeposit.Ok.deposit_id)).toHaveProperty("Ok.Complete");

    const governanceDeposit: any = await deposit(92);
    bridge.actor.setPrincipal(thirdParty);
    expect(await confirmDeposit(bridge, governanceDeposit.Ok.deposit_id)).toEqual({ Err: { Unauthorized: null } });
    bridge.actor.setPrincipal(Principal.anonymous());
    expect(await confirmDeposit(bridge, governanceDeposit.Ok.deposit_id)).toEqual({ Err: { AnonymousCaller: null } });
    bridge.actor.setPrincipal(runtimePrincipal);
    expect(await confirmDeposit(bridge, governanceDeposit.Ok.deposit_id)).toHaveProperty("Ok.Complete");

    expect(await (bridge.actor as any).rotate_pause_principal({ pause_principal: pausePrincipal })).toHaveProperty("Ok");
    bridge.actor.setPrincipal(owner);
    const pauseDeposit: any = await deposit(93);
    bridge.actor.setPrincipal(pausePrincipal);
    expect(await confirmDeposit(bridge, pauseDeposit.Ok.deposit_id)).toHaveProperty("Ok.Complete");
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

  it("serializes Continue calls for unrelated records with the global settlement lease", async () => {
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
    expect(results.filter((result) => "Ok" in result)).toHaveLength(1);
    expect(results.filter((result) => "Err" in result && "Busy" in result.Err)).toHaveLength(1);
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
    const depositActor = pic!.createActor(bridgeIdl, bridge.canisterId);
    depositActor.setPrincipal(runtimePrincipal);

    const standards: any = await (bridge.actor as any).icrc10_supported_standards();
    expect(standards).toEqual([{ name: "ICRC-21", url: "https://github.com/dfinity/ICRC/blob/main/ICRCs/ICRC-21/ICRC-21.md" }]);
    const config: any = await (bridge.actor as any).get_public_config();
    expect(config.base_chain_id).toBe(8453n);
    expect(config.schema_version).toBe(16);
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
    expect(withdrawalConsent.Ok.consent_message.GenericDisplayMessage).toContain("Base burn is irreversible");
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
    await confirmDeposit(bridge, result.Ok.deposit_id);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(stored[0].service_fee).toBe(1n);
    expect(stored[0].net_amount).toBe(99n);
    expect(phaseName(stored[0].state)).toBe("Minted");
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
    await (evm.actor as any).set_eth_balance(20_000_001n);
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
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
  });

  it("refreshes at most one stale Mint snapshot per request and fails closed", async () => {
    const { ledger, evm, bridge } = await setup();
    const seed: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    await confirmDeposit(bridge, seed.Ok.deposit_id);
    expect(phaseName((await (bridge.actor as any).get_deposit(seed.Ok.deposit_id))[0].state)).toBe("Minted");

    await pic!.advanceTime(61_000);
    await (evm.actor as any).set_finalized_block_sequence([98n, 100n]);
    const stale: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    expect(stale).toEqual({ Err: { BaseObservationUnavailable: null } });
    await pic!.advanceTime(61_000);
    const refreshed: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    expect(refreshed).toHaveProperty("Ok");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(2);

    await pic!.advanceTime(61_000);
    await (evm.actor as any).set_finalized_block_sequence([98n, 98n, 98n, 98n, 98n]);
    const unavailable: any = await (bridge.actor as any).request_deposit({ owner_sequence: 2n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 10n, max_service_fee: 10n });
    expect(unavailable).toEqual({ Err: { BaseObservationUnavailable: null } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(2);
  });

  it("reuses a finalized Base Mint snapshot within the admission TTL", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
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
    const { ledger, bridge, init, runtimePrincipal } = await setup();
    const args = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n };
    const depositDeferred = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    depositDeferred.setPrincipal(runtimePrincipal);
    const pauseDeferred = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    pauseDeferred.setPrincipal(init.pause_principal);
    const awaitDeposit = await depositDeferred.request_deposit(args);
    const awaitPause = await pauseDeferred.pause_new_deposits();
    const [pending] = await Promise.all([awaitDeposit(), awaitPause()]);
    expect(pending).toEqual({ Err: { DepositsPaused: null } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);

    await (bridge.actor as any).resume_new_deposits();
    const accepted: any = await (bridge.actor as any).request_deposit(args);
    expect(accepted).toHaveProperty("Ok");
    bridge.actor.setPrincipal(init.pause_principal);
    await (bridge.actor as any).pause_new_deposits();
    bridge.actor.setPrincipal(runtimePrincipal);
    const replay: any = await (bridge.actor as any).request_deposit(args);
    expect(Array.from(replay.Ok.deposit_id)).toEqual(Array.from(accepted.Ok.deposit_id));
  });

  it("accepts a finalized committed withdrawal and pays ICP without another Base transaction", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(6);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
    const ingested = await notifyFixtureWithdrawal(bridge);
    expect(Array.from(ingested.Ok.Ingested.withdrawal_id)).toEqual(Array.from(id));
    expect(phaseName((await (bridge.actor as any).get_withdrawal(id))[0].state)).toBe("Paid");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    await (ledger.actor as any).set_ledger_fee_available(false);
    await (evm.actor as any).set_observed_transaction(new Uint8Array(32).fill(9), new Uint8Array(20).fill(1), new Uint8Array(20).fill(0x22), 99n);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
    const duplicate: any = await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    expect(Array.from(duplicate.Ok.Duplicate.withdrawal_id)).toEqual(Array.from(id));
    expect((await (bridge.actor as any).get_bridge_status()).counts.withdrawals).toBe(1n);
    const withdrawal: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(withdrawal[0].state)).toBe("Paid");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    const broadcasts = await (evm.actor as any).broadcast_transactions();
    expect(broadcasts).toHaveLength(0);
  });

  it("never calls the Ledger before the user withdrawal reaches the finalized head", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xa0);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
    await (evm.actor as any).set_finalized_block_sequence([98n]);
    const premature: any = await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    expect(premature).toHaveProperty("Err.TransactionNotConfirmed");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    await (evm.actor as any).set_finalized_block_sequence([100n]);
    const notified: any = await notifyFixtureWithdrawal(bridge);
    expect(notified.Ok.Ingested.settlement[0]).toHaveProperty("Complete");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    expect(phaseName((await (bridge.actor as any).get_withdrawal(id))[0].state)).toBe("Paid");
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
      await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
      await (evm.actor as any).set_receipt_mode(mode);
      const result: any = await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
      expect(result).toHaveProperty(`Err.${error}`);
      await advanceTimeWithoutSettlement(2);
      expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
      await pic!.tearDown();
      pic = await PocketIc.create(serverUrl, { nns: { state: { type: SubnetStateType.New } }, fiduciary: { state: { type: SubnetStateType.New } } });
    }
  });

  it("binds withdrawal state reads to the current canonical finalized block with EIP-1898", async () => {
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
      max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n,
    }]);

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) }))
      .toHaveProperty("Ok.Ingested");
    expect(Array.from(await (evm.actor as any).pinned_eth_call_block_numbers())).toEqual([100n, 100n]);
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
      max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n,
    }]);
    await (evm.actor as any).set_chain_id_mode(mode);

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) }))
      .toHaveProperty(`Err.${error}`);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(Array.from(await (evm.actor as any).pinned_eth_call_block_numbers())).toEqual([]);
  });

  it.each([
    { mode: { FinalizedUnavailable: null }, error: "RpcUnavailable", tag: 0x9c },
    { mode: { FinalizedInconsistent: null }, error: "RpcInconsistent", tag: 0x9e },
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
      max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n,
    }]);
    await (evm.actor as any).set_block_mode(mode);

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) }))
      .toHaveProperty(`Err.${error}`);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  });

  it("rejects a non-committed old receipt before any Ledger release call", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xa1);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
    await (evm.actor as any).set_withdrawal_status(0);

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toEqual({ Err: { BaseStateMismatch: null } });
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  });

  it("rejects signer rotation between the receipt and finalized Base state read before Ledger", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xa2);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
    expect(await (evm.actor as any).set_bridge_signer(new Uint8Array(20).fill(0xaa))).toHaveProperty("Ok");

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toEqual({ Err: { BridgeSignerMismatch: null } });
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  });

  it("rejects non-confirmed and wrong-owner notifications and ingests one concurrent replay", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(86);
    await (evm.actor as any).set_withdrawal([{ id, owner: Principal.selfAuthenticating(new Uint8Array(32).fill(8)).toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
    bridge.actor.setPrincipal(Principal.selfAuthenticating(new Uint8Array(32).fill(9)));
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Err.OwnerMismatch");
    bridge.actor.setPrincipal(runtimePrincipal);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
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
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Ok.Ingested");
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 101n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Err.WithdrawalConflict");
  });

  it("returns the observed error for forty invalid notifications without notification quota", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(88);
    await (evm.actor as any).set_observed_transaction(new Uint8Array(32).fill(9), new Uint8Array(20).fill(1), new Uint8Array(20).fill(0x22), 101n);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
    const callsBefore = await (evm.actor as any).receipt_call_count();
    for (let attempt = 0; attempt < 40; attempt += 1) {
      expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Err.TransactionNotConfirmed");
    }
    expect(await (evm.actor as any).receipt_call_count()).toBe(callsBefore + 40n);
  });

  it("returns a ledger fee failure without storing or scheduling the withdrawal", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(87);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
    await (ledger.actor as any).set_ledger_fee_available(false);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Err.LedgerFeeUnavailable");
    await advanceTimeWithoutSettlement(2);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  });

  it("persists a fee-guarded withdrawal without release and resumes the same record after fee recovery", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xb7);
    await (ledger.actor as any).set_ledger_fee(11n);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);

    const guarded: any = await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    expect(guarded).toEqual({ Err: { LedgerFeeExceedsServiceFee: { ledger_fee: 11n, charged_service_fee: 10n } } });
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    const blocked: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(blocked[0].state)).toBe("Observed");
    expect(blocked[0].last_settlement_stop_reason).toEqual(["LedgerFeeExceedsServiceFee"]);
    expect((await (bridge.actor as any).get_bridge_status()).withdrawal_fee_guard_active).toBe(true);

    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    await (ledger.actor as any).set_ledger_fee(1n);
    expect(await (bridge.actor as any).continue_withdrawal(id)).toHaveProperty("Ok.Complete");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    const paid: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(paid[0].state)).toBe("Paid");
    expect(paid[0].ledger_fee).toBe(1n);
    expect((await (bridge.actor as any).get_bridge_status()).withdrawal_fee_guard_active).toBe(false);
  });

  it("continues an ambiguous Withdrawal release from reconciled Hold to Paid", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    const id = new Uint8Array(32).fill(46);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
    await notifyFixtureWithdrawal(bridge);
    const held: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(held[0].state)).toBe("ReconciliationHold");
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    expect(await (bridge.actor as any).continue_withdrawal(id)).toHaveProperty("Ok.Complete");
    const released: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(released[0].state)).toBe("Paid");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("stops an unexpected BadFee without changing the Withdrawal transfer identity", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xb1);
    await (ledger.actor as any).set_ledger_fee(1n);
    await (ledger.actor as any).set_ledger_mode({ BadFee: null });
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
    await notifyFixtureWithdrawal(bridge);

    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    const stopped: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(stopped[0].state)).toBe("ReleasePending");
    expect(stopped[0].ledger_fee).toBe(1n);
    expect(stopped[0].last_settlement_stop_reason[0]).toContain("BadFee");

    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    expect(await (bridge.actor as any).continue_withdrawal(id)).toHaveProperty("Ok.Complete");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(2n);
    const paid: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(paid[0].state)).toBe("Paid");
    expect(paid[0].ledger_fee).toBe(1n);
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });


  it("does not reprice or cancel after an ambiguous Ledger release", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xb5);
    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100n, max_service_fee: 10n, charged_service_fee: 10n, amount_out: 90n }]);
    await notifyFixtureWithdrawal(bridge);
    expect(phaseName((await (bridge.actor as any).get_withdrawal(id))[0].state)).toBe("ReconciliationHold");

    await (ledger.actor as any).set_ledger_fee(2n);
    await (ledger.actor as any).set_ledger_mode({ BadFee: null });
    const retry: any = await (bridge.actor as any).continue_withdrawal(id);
    expect(retry).toHaveProperty("Ok.Stopped.reason.LedgerRejected");
    expect(phaseName((await (bridge.actor as any).get_withdrawal(id))[0].state)).toBe("ReconciliationHold");
  });


  it("continues an ambiguous deposit from reconciled Hold to Mint", async () => {
    const { ledger, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
    expect(phaseName(result.Ok.state)).toBe("ReconciliationHold");
    const before: any = await (bridge.actor as any).get_bridge_status();
    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    const after: any = await (bridge.actor as any).get_bridge_status();
    expect(after.counts.reconciliation_holds).toBe(before.counts.reconciliation_holds);
    expect(after.counts.reconciliation_holds).toBe(1n);
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    expect(await (bridge.actor as any).continue_deposit(result.Ok.deposit_id)).toHaveProperty("Ok.Submitted");
    await confirmDeposit(bridge, result.Ok.deposit_id);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("Minted");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("does not retry a retryable deposit pull in the admission call", async () => {
    const { ledger, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode({ TemporarilyUnavailable: null });

    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });

    expect(result).toHaveProperty("Ok.settlement.0.Stopped");
    expect(phaseName(result.Ok.state)).toBe("PullPending");
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
    expect(await (bridge.actor as any).continue_deposit(result.Ok.deposit_id)).toHaveProperty("Err.ConfirmationRequired");
    await advanceClock(20);
    expect(await (evm.actor as any).receipt_call_count()).toBe(receiptCallsBeforeReplay + 1n);
    await (evm.actor as any).set_receipt_mode({ RpcFailure: null });
    expect(await confirmDeposit(bridge, result.Ok.deposit_id)).toHaveProperty("Ok.Stopped");
    await (evm.actor as any).set_receipt_mode({ Missing: null });
    expect(await confirmDeposit(bridge, result.Ok.deposit_id)).toHaveProperty("Ok.WaitingForConfirmation");
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("MintPending");
  });

  it("recovers an ambiguously accepted replacement across restart and accepts the original generation winning", async () => {
    const { evm, bridge } = await setup();
    await (evm.actor as any).set_receipt_mode({ Missing: null });
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
    const first: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    const originalHash = first[0].base_confirmation[0].Submitted.transaction_hash;
    expect(first[0].automatic_progress[0]).toHaveProperty("phase.Confirmation");
    expect((await (bridge.actor as any).get_bridge_status()).settlement_scheduler.scheduled).toBe(1n);

    await advanceTimeWithoutSettlement(5);
    let broadcasts: Uint8Array[] = await (evm.actor as any).broadcast_transactions();
    expect(broadcasts).toHaveLength(2);
    expect(Buffer.from(broadcasts[1])).toEqual(Buffer.from(broadcasts[0]));

    await advanceTimeWithoutSettlement(24);
    await (evm.actor as any).set_broadcast_inconsistent_after_accepts(1);
    await advanceTimeWithoutSettlement(1);
    broadcasts = await (evm.actor as any).broadcast_transactions();
    expect(broadcasts.length).toBeGreaterThanOrEqual(7);
    expect(Buffer.from(broadcasts.at(-1)!)).not.toEqual(Buffer.from(broadcasts[0]));
    const pending: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(pending[0].base_confirmation[0].Submitted.transaction_hash).toEqual(originalHash);

    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    await advanceTimeWithoutSettlement(1);
    broadcasts = await (evm.actor as any).broadcast_transactions();
    expect(Buffer.from(broadcasts.at(-1)!)).toEqual(Buffer.from(broadcasts.at(-2)!));
    const replaced: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(replaced[0].base_confirmation[0].Submitted.transaction_hash).not.toEqual(originalHash);

    await (evm.actor as any).set_receipt_mode({ Confirmed: null });
    expect(await (bridge.actor as any).confirm_deposit({
      settlement_id: result.Ok.deposit_id,
      transaction_hash: originalHash,
      receipt_block_number: 99n,
      observed_finalized_block_number: 100n,
    })).toHaveProperty("Ok.Complete");
    expect(phaseName((await (bridge.actor as any).get_deposit(result.Ok.deposit_id))[0].state)).toBe("Minted");
  });

  it("validates frontend confirmation evidence before any Base outcall", async () => {
    const { evm, bridge } = await setup();
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    const transactionHash = stored[0].base_confirmation[0].Submitted.transaction_hash;
    const callsBefore = await (evm.actor as any).receipt_call_count();
    expect(await (bridge.actor as any).confirm_deposit({ settlement_id: result.Ok.deposit_id, transaction_hash: new Uint8Array(32).fill(0xff), receipt_block_number: 99n, observed_finalized_block_number: 100n })).toHaveProperty("Err.TransactionMismatch");
    expect(await (bridge.actor as any).confirm_deposit({ settlement_id: result.Ok.deposit_id, transaction_hash: transactionHash, receipt_block_number: 101n, observed_finalized_block_number: 100n })).toHaveProperty("Err.InvalidConfirmationObservation");
    expect(await (evm.actor as any).receipt_call_count()).toBe(callsBefore);
  });

  it("does not retry an explicit confirmation after an RPC failure", async () => {
    const { evm, bridge } = await setup();
    await (evm.actor as any).set_receipt_mode({ RpcFailure: null });
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
    await confirmDeposit(bridge, result.Ok.deposit_id);
    const stopped: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(stopped[0].last_settlement_stop_reason).toEqual(["Base RPC unavailable"]);
    expect(stopped[0].automatic_progress).toEqual([]);
    expect(await (evm.actor as any).receipt_call_count()).toBe(1n);
    await advanceClock(60);
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

  it("terminalizes a finalized EVM revert, pauses deposits, and never rebroadcasts it", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    await (evm.actor as any).set_receipt_mode({ Reverted: null });
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
    await confirmDeposit(bridge, result.Ok.deposit_id);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("MintReverted");
    const status: any = await (bridge.actor as any).get_bridge_status();
    expect(status.deposits_paused).toBe(true);
    expect(status.counts.unresolved_evm_reverts).toBe(1n);
    expect(await (bridge.actor as any).resume_new_deposits()).toEqual({ Err: { UnresolvedEvmRevert: null } });
    const audit: any = await (bridge.actor as any).get_audit_events(0n, 100);
    const reverted = audit.Ok.events.find((event: any) => "EvmOperationReverted" in event.kind);
    expect(reverted.kind.EvmOperationReverted.kind).toEqual({ MintDeposit: null });
    expect(reverted.kind.EvmOperationReverted.transaction_hash).toHaveLength(32);
    expect(reverted.kind.EvmOperationReverted.finalized_head_block_number).toBeGreaterThan(0n);
    const before = await (evm.actor as any).broadcast_transactions();
    expect(before).toHaveLength(1);
    await advanceTimeWithoutSettlement(4);
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(1);
    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    const reopened: any = await (bridge.actor as any).get_bridge_status();
    expect(reopened.deposits_paused).toBe(true);
    expect(reopened.counts.unresolved_evm_reverts).toBe(1n);

    const operationId = reverted.kind.EvmOperationReverted.operation_id;
    const rpcCallsBeforeUnauthorized = await (evm.actor as any).eth_call_count();
    bridge.actor.setPrincipal(Principal.selfAuthenticating(new Uint8Array(32).fill(77)));
    expect(await (bridge.actor as any).recover_mint_revert({
      deposit_id: result.Ok.deposit_id, reverted_operation_id: operationId,
    })).toEqual({ Err: { Unauthorized: null } });
    expect(await (evm.actor as any).eth_call_count()).toBe(rpcCallsBeforeUnauthorized);

    bridge.actor.setPrincipal(runtimePrincipal);
    await (evm.actor as any).set_receipt_mode({ Confirmed: null });
    const recovery: any = await (bridge.actor as any).recover_mint_revert({
      deposit_id: result.Ok.deposit_id, reverted_operation_id: operationId,
    });
    expect(recovery).toHaveProperty("Ok.Enqueued");
    expect(recovery.Ok.Enqueued.replacement_operation_id).not.toBe(operationId);
    expect(await (bridge.actor as any).recover_mint_revert({
      deposit_id: result.Ok.deposit_id, reverted_operation_id: operationId,
    })).toHaveProperty("Ok.AlreadyStarted");
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(2);
    await confirmDeposit(bridge, result.Ok.deposit_id);
    expect(phaseName((await (bridge.actor as any).get_deposit(result.Ok.deposit_id))[0].state)).toBe("Minted");
    expect((await (bridge.actor as any).get_bridge_status()).counts.unresolved_evm_reverts).toBe(0n);
    expect(await (bridge.actor as any).resume_new_deposits()).toHaveProperty("Ok");
  });



  it("pauses only new deposits and allows Governance to resume them", async () => {
    const { bridge, init, runtimePrincipal } = await setup();
    bridge.actor.setPrincipal(init.pause_principal);
    expect(await (bridge.actor as any).pause_new_deposits()).toHaveProperty("Ok");
    bridge.actor.setPrincipal(runtimePrincipal);
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
    for (const id of firstPage.Ok.deposit_ids) {
      await confirmDeposit(bridge, id);
      expect(phaseName((await (bridge.actor as any).get_deposit(id))[0].state)).toBe("Minted");
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
    expect(phaseName(replay.Ok.state)).toBe("Cancelled");

    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    await advanceTimeWithoutSettlement(2);
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);
    const replacement = { ...failed, owner_sequence: 1n };
    expect(await (bridge.actor as any).request_deposit(replacement)).toHaveProperty("Ok");
    status = await (bridge.actor as any).get_bridge_status();
    expect(status.counts.reserved_deposit_mint_amount).toBe(99n);
  });

  it.each([
    ["InsufficientAllowance", { InsufficientAllowance: { allowance: 0n } }],
    ["InsufficientFunds", { InsufficientFunds: { balance: 0n } }],
  ])("cancels a definitive %s pull rejection without creating a ledger transaction", async (_label, mode) => {
    const { ledger, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode(mode);
    const result: any = await (bridge.actor as any).request_deposit({
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 100n,
      max_service_fee: 10n,
    });
    expect(result).toHaveProperty("Ok.settlement.0.Complete");
    const record: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(phaseName(record[0].state)).toBe("Cancelled");
    expect(await (ledger.actor as any).ledger_transactions()).toEqual([]);
  });

  it("serves configured transaction prefixes through the ICRC archive callback", async () => {
    const { ledger, bridge } = await setup();
    await (bridge.actor as any).request_deposit({
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 100n,
      max_service_fee: 10n,
    });
    await (ledger.actor as any).set_archive_prefix_length(1n);
    const page: any = await (ledger.actor as any).get_transactions({ start: 0n, length: 10n });
    expect(page.transactions).toEqual([]);
    expect(page.archived_transactions).toHaveLength(1);
    expect(page.archived_transactions[0].start).toBe(0n);
    expect(page.archived_transactions[0].length).toBe(1n);
    const archived: any = await (ledger.actor as any).get_archive_transactions({ start: 0n, length: 10n });
    expect(archived.transactions).toHaveLength(1);
  });

  it("keeps an ambiguous deposit nonterminal until the Index watermark reaches the Ledger tip", async () => {
    const { ledger, index, bridge } = await setup();
    const first: any = await (bridge.actor as any).request_deposit({
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 100n,
      max_service_fee: 10n,
    });
    expect(first).toHaveProperty("Ok");
    expect(await (ledger.actor as any).ledger_transactions()).toHaveLength(1);

    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    const ambiguous: any = await (bridge.actor as any).request_deposit({
      owner_sequence: 1n,
      base_recipient: new Uint8Array(20).fill(5),
      from_subaccount: [],
      gross_amount: 100n,
      max_service_fee: 10n,
    });
    expect(phaseName(ambiguous.Ok.state)).toBe("ReconciliationHold");

    await (index.actor as any).set_index_synced_blocks([0n]);
    await pic!.advanceTime(24 * 60 * 60 * 1_000 + 1);
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    expect(await (bridge.actor as any).continue_deposit(ambiguous.Ok.deposit_id)).toHaveProperty(
      "Ok.ReconciliationProgress",
    );
    expect(phaseName((await (bridge.actor as any).get_deposit(ambiguous.Ok.deposit_id))[0].state)).toBe(
      "ReconciliationHold",
    );
    expect(await (ledger.actor as any).ledger_transactions()).toHaveLength(1);

    await (index.actor as any).set_index_synced_blocks([1n]);
    expect(await (bridge.actor as any).continue_deposit(ambiguous.Ok.deposit_id)).toHaveProperty(
      "Ok.Complete",
    );
    expect(phaseName((await (bridge.actor as any).get_deposit(ambiguous.Ok.deposit_id))[0].state)).toBe(
      "Cancelled",
    );
    expect(await (ledger.actor as any).ledger_transactions()).toHaveLength(1);
  });

  it("fails a retryable fee payout without trapping its reserve", async () => {
    const { ledger, bridge, runtimePrincipal } = await setup();
    for (const tag of [56, 57]) {
      const deposit: any = await (bridge.actor as any).request_deposit({ owner_sequence: BigInt(tag - 56), base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 100n, max_service_fee: 10n });
      expect(deposit).toHaveProperty("Ok");
    }
    const page: any = await (bridge.actor as any).list_deposit_ids({ owner: runtimePrincipal, before_cursor: [], limit: 20 });
    for (const id of page.Ok.deposit_ids) {
      await confirmDeposit(bridge, id);
      expect(phaseName((await (bridge.actor as any).get_deposit(id))[0].state)).toBe("Minted");
    }
    await (ledger.actor as any).set_ledger_mode({ TemporarilyUnavailable: null });
    const failed: any = await (bridge.actor as any).request_fee_payout(1n);
    expect(failed.Ok.state).toEqual({ Pending: null });
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    const retried: any = await (bridge.actor as any).continue_fee_payout(failed.Ok.id);
    expect(retried).toHaveProperty("Ok.Complete");
  });

  it("keeps large SQLite status and upgrade work bounded and completes controller maintenance", async () => {
    const { bridge, runtimePrincipal } = await setup(false);
    const maintenanceIdl = ({ IDL }: { IDL: any }) => {
      const error = IDL.Variant({
        Unauthorized: IDL.Null,
        InvalidArgument: IDL.Record({ message: IDL.Text }),
        StateChanged: IDL.Null,
        NotStarted: IDL.Null,
        StorageFailure: IDL.Null,
      });
      const validation = IDL.Record({
        complete: IDL.Bool,
        phase: IDL.Text,
        scanned_rows: IDL.Nat64,
      });
      const checksum = IDL.Record({
        complete: IDL.Bool,
        checksum: IDL.Nat64,
        scanned_bytes: IDL.Nat64,
        db_size: IDL.Nat64,
      });
      return IDL.Service({
        seed_storage_test_data: IDL.Func(
          [IDL.Nat64, IDL.Nat16],
          [IDL.Variant({ Ok: IDL.Nat16, Err: error })],
          [],
        ),
        first_prepared_evm_test_id: IDL.Func(
          [],
          [IDL.Variant({ Ok: IDL.Opt(IDL.Nat64), Err: error })],
          ["query"],
        ),
        start_storage_validation: IDL.Func(
          [],
          [IDL.Variant({ Ok: validation, Err: error })],
          [],
        ),
        continue_storage_validation: IDL.Func(
          [IDL.Nat16],
          [IDL.Variant({ Ok: validation, Err: error })],
          [],
        ),
        storage_integrity_check: IDL.Func(
          [],
          [IDL.Variant({ Ok: IDL.Text, Err: error })],
          ["query"],
        ),
        refresh_storage_checksum: IDL.Func(
          [IDL.Nat64],
          [IDL.Variant({ Ok: checksum, Err: error })],
          [],
        ),
      });
    };
    const maintenance: any = pic!.createActor(maintenanceIdl as any, bridge.canisterId);
    maintenance.setPrincipal(runtimePrincipal);
    expect(await maintenance.start_storage_validation()).toHaveProperty("Err.Unauthorized");
    const [controller] = await pic!.getControllers(bridge.canisterId);
    if (controller === undefined) throw new Error("bridge controller is missing");
    maintenance.setPrincipal(controller);

    for (let start = 0; start < 10_000; start += 100) {
      const seeded: any = await maintenance.seed_storage_test_data(BigInt(start), 100);
      expect(seeded.Ok).toBe(100);
    }
    const before: any = await (bridge.actor as any).get_bridge_status();
    expect(before.schema_version).toBe(16);
    expect(before.counts.withdrawals).toBe(10_000n);
    expect(before.counts.pending_evm_operations).toBe(10_000n);
    expect(before.counts.active_evm_payloads).toBe(10_000n);
    expect(before.counts.retained_audit_events).toBe(10_000n);
    expect(before.settlement_scheduler.scheduled).toBe(10_000n);
    expect(before.unpaid_withdrawal_count).toBe(10_000n);
    expect(before.unpaid_withdrawal_amount_out).toBe(900_000n);
    expect((await maintenance.first_prepared_evm_test_id()).Ok).toEqual([0n]);

    const firstId = createHash("sha256")
      .update("KINIC_BRIDGE_STORAGE_SEED_V1")
      .update(Buffer.alloc(8))
      .digest();
    expect(await (bridge.actor as any).get_withdrawal(firstId)).toHaveLength(1);
    await pic!.upgradeCanister({
      canisterId: bridge.canisterId,
      wasm: readFileSync(bridgeWasm),
      arg: IDL.encode([], []),
      sender: controller,
    });
    const after: any = await (bridge.actor as any).get_bridge_status();
    expect(after.schema_version).toBe(16);
    expect(after.counts).toEqual(before.counts);
    expect(after.settlement_scheduler.scheduled).toBe(10_000n);
    expect((await maintenance.first_prepared_evm_test_id()).Ok).toEqual([0n]);
    expect(await (bridge.actor as any).get_withdrawal(firstId)).toHaveLength(1);

    expect(await maintenance.start_storage_validation()).toHaveProperty("Ok");
    for (;;) {
      const continued: any = await maintenance.continue_storage_validation(100);
      if (!("Ok" in continued)) throw new Error(`validation failed: ${JSON.stringify(continued)}`);
      if (continued.Ok.complete) break;
    }
    expect((await maintenance.storage_integrity_check()).Ok).toBe("ok");
    for (;;) {
      const refreshed: any = await maintenance.refresh_storage_checksum(4_194_304n);
      if (!("Ok" in refreshed)) throw new Error(`checksum failed: ${JSON.stringify(refreshed)}`);
      if (refreshed.Ok.complete) break;
    }
  }, 1_200_000);
});
