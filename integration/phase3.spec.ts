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
function debugJson(value: unknown): string {
  return JSON.stringify(value, (_key, item) => typeof item === "bigint" ? `${item}n` : item);
}
describe("Phase 3 PocketIC saga", () => {
  let server: ChildProcess | undefined;
  let pic: PocketIc | undefined;
  let serverUrl = "";

  async function setup(activate = true, governanceRebroadcastAfterSeconds = 300n) {
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
    const feeRecipientPrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(55));
    const governanceCheckIntervalSeconds = governanceRebroadcastAfterSeconds < 60n ? 30n : 60n;
    const init = { ledger_canister_id: ledger.canisterId, index_canister_id: index.canisterId, evm_rpc_canister_id: evm.canisterId, custom_evm_rpc_urls: [], base_chain_id: 8453n, bridge_contract: new Uint8Array(20).fill(1), timelock_contract: new Uint8Array(20).fill(2), ecdsa_key_name: "key_1", ecdsa_derivation_path: [], governance_ecdsa_derivation_path: [new TextEncoder().encode("governance-operator")], deposit_rate_limit_window_seconds: 60n, deposit_rate_limit_global: 30, deposit_rate_limit_per_principal: 3, settlement_rate_limit_window_seconds: 3_600n, settlement_rate_limit_global: 60, settlement_rate_limit_per_principal: 30, settlement_rate_limit_per_record: 3, governance_evm_fee: { gas_limit_ceiling: 500_000n, max_fee_per_gas_ceiling: 200_000_000_000n, max_priority_fee_per_gas_ceiling: 10_000_000_000n, l1_fee_per_transaction_ceiling_wei: 10_000_000_000_000_000n, quote_validity_seconds: 90n, gas_limit_multiplier_bps: 13_000, base_fee_multiplier_bps: 60_000, l1_fee_multiplier_bps: 15_000 }, governance_evm_liveness: { check_interval_seconds: governanceCheckIntervalSeconds, rebroadcast_after_seconds: governanceRebroadcastAfterSeconds, replacement_after_seconds: 1_800n, max_replacements: 3, fee_bump_bps: 1_250 }, governance_eth_floor_wei: 1n, cycles_floor: 1n, settlement_cycle_ceiling: 1n, governance_principal: runtimePrincipal, pause_principal: Principal.selfAuthenticating(new Uint8Array(32).fill(34)), fee_recipient: { owner: feeRecipientPrincipal, subaccount: [] } };
    const bridge = await pic!.setupCanister({ idlFactory: bridgeIdl, wasm: readFileSync(bridgeWasm), arg: IDL.encode([bridgeInit], [init]), cycles: 500_000_000_000_000n, targetSubnetId: subnet.id });
    expect(await (bridge.actor as any).initialize_public_config()).toHaveProperty("Ok");
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
  async function advancePastAutomaticRetry(minutes = 21) {
    await pic!.advanceTime(minutes * 60_000 + 1);
  }
  async function continueDeposit(bridge: any, depositId: Uint8Array) {
    await pic!.advanceTime(6 * 60_000 + 1);
    return bridge.actor.continue_deposit(depositId);
  }
  async function continueWithdrawal(bridge: any, withdrawalId: Uint8Array) {
    await pic!.advanceTime(6 * 60_000 + 1);
    return bridge.actor.continue_withdrawal(withdrawalId);
  }
  async function awaitMintAuthorization(bridge: any, depositId: Uint8Array) {
    const attempts: any[] = [];
    for (let attempt = 0; attempt < 8; attempt += 1) {
      const stored: any = await bridge.actor.get_deposit(depositId);
      const authorization = stored[0]?.mint_authorization?.[0];
      if (authorization?.signature?.[0] !== undefined) return authorization;
      const advanced = await continueDeposit(bridge, depositId);
      attempts.push({ advanced, state: stored[0]?.state, automatic_progress: stored[0]?.automatic_progress });
      if ("Err" in advanced && "RateLimited" in advanced.Err) {
        await pic!.advanceTime((Number(advanced.Err.RateLimited.retry_after_seconds) + 1) * 1_000);
        await pic!.tick(5);
        continue;
      }
      if (!("Ok" in advanced) && !("Busy" in advanced.Err) && !("AutomaticProgressPending" in advanced.Err)) {
        throw new Error(`authorization progress failed: ${debugJson(advanced)}`);
      }
      if (!("Ok" in advanced)) await pic!.tick(5);
    }
    throw new Error(`deposit has no signed Mint Authorization: ${debugJson(attempts)}`);
  }
  async function mintAuthorizedDeposit(bridge: any, evm: any, depositId: Uint8Array) {
    const authorization = await awaitMintAuthorization(bridge, depositId);
    const transactionHash = new Uint8Array(32).fill(0x42);
    await evm.actor.set_observed_transaction(
      transactionHash,
      authorization.verifying_contract,
      new Uint8Array(20).fill(0x77),
      authorization.finalized_block_number,
    );
    await evm.actor.set_processed_deposit(true);
    await evm.actor.set_mint_log([{
      deposit_id: authorization.deposit_id,
      recipient: authorization.recipient,
      authorization_digest: authorization.digest,
      gross_amount: authorization.gross_amount,
      charged_service_fee: authorization.charged_service_fee,
      minted_amount: authorization.gross_amount - authorization.charged_service_fee,
      transaction_hash: transactionHash,
    }]);
    await evm.actor.set_block_timestamp(authorization.deadline + 1n);
    const result = await continueDeposit(bridge, depositId);
    // The mock exposes one processed flag and one log list rather than a
    // deposit-keyed contract state. Do not leak one completed Mint into the
    // next deposit's preflight.
    await evm.actor.set_processed_deposit(false);
    await evm.actor.set_mint_log([]);
    return result;
  }
  async function expireUnusedAuthorization(bridge: any, evm: any, depositId: Uint8Array, timestampOffset = 1n) {
    const authorization = await awaitMintAuthorization(bridge, depositId);
    await evm.actor.set_processed_deposit(false);
    await evm.actor.set_mint_log([]);
    await evm.actor.set_block_timestamp(authorization.deadline + timestampOffset);
    return {
      authorization,
      result: await continueDeposit(bridge, depositId),
    };
  }
  async function assertAuthorizationRefunded(bridge: any, evm: any, depositId: Uint8Array) {
    const expired = await expireUnusedAuthorization(bridge, evm, depositId);
    expect(expired.result).toHaveProperty("Ok.Complete");
    const stored: any = await bridge.actor.get_deposit(depositId);
    expect(phaseName(stored[0].state)).toBe("Refunded");
    expect(stored[0].refund[0].reason).toEqual({ AuthorizationExpired: null });
    return expired;
  }
  async function requestDefaultDeposit(bridge: any, ownerSequence = 0n, recipientTag = 4) {
    return bridge.actor.request_deposit({
      owner_sequence: ownerSequence,
      base_recipient: new Uint8Array(20).fill(recipientTag),
      from_subaccount: [],
      gross_amount: 20_000n,
      max_service_fee: 10n,
    });
  }
  async function notifyFixtureWithdrawal(bridge: any) {
    const result = await bridge.actor.notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    expect(result).toHaveProperty("Ok");
    expect(result.Ok).toHaveProperty("Ingested");
    const withdrawalId = result.Ok.Ingested.withdrawal_id;
    const advanced = await continueWithdrawal(bridge, withdrawalId);
    expect(advanced).toHaveProperty("Ok");
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

  it("persists one idempotent Deposit through ledger pull, Mint Authorization, and finalized exact Mint evidence", async () => {
    const { bridge, ledger, evm } = await setup();

    const request = {
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 20_000n,
      max_service_fee: 10n,
    };
    const ledgerCallsBefore = await (ledger.actor as any).ledger_transfer_calls();
    const baseCallsBefore = await (evm.actor as any).eth_call_count();
    const first: any = await (bridge.actor as any).request_deposit(request);
    if (!("Ok" in first)) {
      throw new Error(`request_deposit failed: ${JSON.stringify(first)}`);
    }
    expect(phaseName(first.Ok.state)).toBe("EscrowedUnquoted");
    expect(await (bridge.actor as any).get_deposit(first.Ok.deposit_id)).toHaveProperty("0.state.EscrowedUnquoted");
    expect(await (bridge.actor as any).get_bridge_status()).toHaveProperty("settlement_scheduler.scheduled", 1n);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(ledgerCallsBefore + 1n);
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(1);
    expect(await (evm.actor as any).eth_call_count()).toBeGreaterThan(baseCallsBefore);
    expect(await mintAuthorizedDeposit(bridge, evm, first.Ok.deposit_id)).toHaveProperty("Ok.Complete");
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
    const request = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n };
    const first: any = await (bridge.actor as any).request_deposit(request);
    expect(first).toHaveProperty("Ok");
    expect(first.Ok.owner_sequence).toBe(0n);
    expect(await (bridge.actor as any).get_next_deposit_sequence(runtimePrincipal)).toBe(1n);
    await continueDeposit(bridge, first.Ok.deposit_id);
    const replay: any = await (bridge.actor as any).request_deposit(request);
    expect(Array.from(replay.Ok.deposit_id)).toEqual(Array.from(first.Ok.deposit_id));
    expect(await (bridge.actor as any).request_deposit({ ...request, gross_amount: 20_001n })).toEqual({ Err: { DepositConflict: null } });
    expect(await (bridge.actor as any).request_deposit({ ...request, owner_sequence: 2n })).toEqual({ Err: { SequenceMismatch: { expected: 1n } } });
  });

  it("rejects gross amounts at or below the fixed refund fee before record, sequence, or ledger use", async () => {
    const { ledger, bridge, runtimePrincipal } = await setup();
    const request = (gross_amount: bigint) => ({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount, max_service_fee: 10n });
    expect(await (bridge.actor as any).request_deposit(request(10_000n))).toHaveProperty("Err.InvalidRequest");
    expect(await (bridge.actor as any).request_deposit(request(9_999n))).toHaveProperty("Err.InvalidRequest");
    expect(await (bridge.actor as any).get_next_deposit_sequence(runtimePrincipal)).toBe(0n);
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(0);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
  });

  it("allows an owner to manually start safe expiry reconciliation but never refunds at the deadline boundary", async () => {
    const { bridge, evm } = await setup();
    const owner = Principal.selfAuthenticating(new Uint8Array(32).fill(31));
    const thirdParty = Principal.selfAuthenticating(new Uint8Array(32).fill(32));

    bridge.actor.setPrincipal(owner);
    const deposit: any = await requestDefaultDeposit(bridge);
    const authorization = await awaitMintAuthorization(bridge, deposit.Ok.deposit_id);
    await evm.actor.set_block_timestamp(authorization.deadline);
    const boundary: any = await continueDeposit(bridge, deposit.Ok.deposit_id);
    expect(boundary).toHaveProperty("Ok.Deferred");
    let stored: any = await bridge.actor.get_deposit(deposit.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("AuthorizationAvailable");
    expect(stored[0].refund).toEqual([]);

    bridge.actor.setPrincipal(thirdParty);
    expect(await continueDeposit(bridge, deposit.Ok.deposit_id)).toEqual({ Err: { Unauthorized: null } });
    bridge.actor.setPrincipal(Principal.anonymous());
    expect(await continueDeposit(bridge, deposit.Ok.deposit_id)).toEqual({ Err: { AnonymousCaller: null } });
    bridge.actor.setPrincipal(owner);
    await evm.actor.set_block_timestamp(authorization.deadline + 1n);
    expect(await continueDeposit(bridge, deposit.Ok.deposit_id)).toHaveProperty("Ok.Complete");
    stored = await bridge.actor.get_deposit(deposit.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("Refunded");
  });

  it("reauthorizes governance rebroadcasts after RPC awaits and preserves the signed transaction across upgrade", async () => {
    const { bridge, evm, runtimePrincipal } = await setup(true, 30n);
    const oldPausePrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(34));
    const currentPausePrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(35));
    const pauseAction = { PauseDepositMints: null };

    await (evm.actor as any).set_broadcast_inconsistent_after_accepts(1);
    bridge.actor.setPrincipal(oldPausePrincipal);
    expect(await (bridge.actor as any).submit_base_governance_action(pauseAction)).toHaveProperty("Err.BroadcastAmbiguous");
    const initialBroadcasts: Uint8Array[] = await (evm.actor as any).broadcast_transactions();
    expect(initialBroadcasts).toHaveLength(1);

    await pic!.upgradeCanister({
      canisterId: bridge.canisterId,
      wasm: readFileSync(bridgeWasm),
      arg: IDL.encode([], []),
    });
    await pic!.advanceTime(30_001);

    await (evm.actor as any).set_chain_id_mode({ Inconsistent: null });
    const oldPauseActor = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    oldPauseActor.setPrincipal(oldPausePrincipal);
    const retryAsOldPause = await oldPauseActor.submit_base_governance_action(pauseAction);
    const governanceActor = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    governanceActor.setPrincipal(runtimePrincipal);
    const rotatePause = await governanceActor.rotate_pause_principal({ pause_principal: currentPausePrincipal });
    const [oldPauseResult, rotationResult] = await Promise.all([retryAsOldPause(), rotatePause()]);
    expect(rotationResult).toHaveProperty("Ok");
    expect(oldPauseResult).toEqual({ Err: { Unauthorized: null } });
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(1);

    bridge.actor.setPrincipal(currentPausePrincipal);
    expect(await (bridge.actor as any).submit_base_governance_action(pauseAction)).toHaveProperty("Err.BroadcastAmbiguous");
    const rebroadcasts: Uint8Array[] = await (evm.actor as any).broadcast_transactions();
    expect(rebroadcasts).toHaveLength(2);
    expect(Buffer.from(rebroadcasts[1])).toEqual(Buffer.from(rebroadcasts[0]));
  });

  it("serializes concurrent Continue calls with automatic progress for the same record", async () => {
    const { bridge, runtimePrincipal } = await setup();
    const deposit: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    const deferred = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    deferred.setPrincipal(runtimePrincipal);
    await pic!.advanceTime(6 * 60_000 + 1);
    const first = await deferred.continue_deposit(deposit.Ok.deposit_id);
    const second = await deferred.continue_deposit(deposit.Ok.deposit_id);
    const results: any[] = await Promise.all([first(), second()]);
    expect(results.filter((result) => "Err" in result && "Busy" in result.Err)).toHaveLength(1);
    expect(
      results.filter(
        (result) =>
          "Ok" in result
          || ("Err" in result && "AutomaticProgressPending" in result.Err),
      ),
    ).toHaveLength(1);
  });

  it("does not let one unrelated record block all public manual progress", async () => {
    const { bridge, runtimePrincipal } = await setup();
    const first: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    const second: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(5), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    expect(first).toHaveProperty("Ok");
    expect(second).toHaveProperty("Ok");
    const deferred = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    deferred.setPrincipal(runtimePrincipal);
    await pic!.advanceTime(6 * 60_000 + 1);
    const continueFirst = await deferred.continue_deposit(first.Ok.deposit_id);
    const continueSecond = await deferred.continue_deposit(second.Ok.deposit_id);
    const results: any[] = await Promise.all([continueFirst(), continueSecond()]);
    expect(results.filter((result) => "Ok" in result)).toHaveLength(1);
    expect(
      results.filter(
        (result) =>
          "Ok" in result
          || ("Err" in result
            && ("AutomaticProgressPending" in result.Err || "Busy" in result.Err)),
      ),
    ).toHaveLength(2);
  });

  it("binds a selected subaccount, exposes public configuration, consent, and owner history", async () => {
    const { bridge, init, runtimePrincipal } = await setup();
    const selectedSubaccount = new Uint8Array(32).fill(8);
    const request = {
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(9),
      from_subaccount: [selectedSubaccount],
      gross_amount: 20_000n,
      max_service_fee: 10n,
    };
    const depositActor = pic!.createActor(bridgeIdl, bridge.canisterId);
    depositActor.setPrincipal(runtimePrincipal);

    const standards: any = await (bridge.actor as any).icrc10_supported_standards();
    expect(standards).toEqual([{ name: "ICRC-21", url: "https://github.com/dfinity/ICRC/blob/main/ICRCs/ICRC-21/ICRC-21.md" }]);
    const config: any = await (bridge.actor as any).get_public_config();
    expect(config.base_chain_id).toBe(8453n);
    expect(config.schema_version).toBe(25);
    expect(config.ledger_canister_id.toText()).toBe(init.ledger_canister_id.toText());
    expect(config.ledger_fee).toBe(10_000n);
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

  it("publishes rotated administrator configuration and preserves it across reopen", async () => {
    const { bridge, init, runtimePrincipal } = await setup(false);
    const nextPausePrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(65));
    const nextFeeRecipient = Principal.selfAuthenticating(new Uint8Array(32).fill(66));

    expect(await (bridge.actor as any).rotate_pause_principal({
      pause_principal: nextPausePrincipal,
    })).toHaveProperty("Ok");
    expect(await (bridge.actor as any).rotate_fee_recipient({
      owner: nextFeeRecipient,
      subaccount: [],
    })).toHaveProperty("Ok");

    const rotated: any = await (bridge.actor as any).get_public_config();
    expect(rotated.pause_principal.toText()).toBe(nextPausePrincipal.toText());
    expect(rotated.fee_recipient.owner.toText()).toBe(nextFeeRecipient.toText());

    bridge.actor.setPrincipal(init.pause_principal);
    expect(await (bridge.actor as any).pause_new_deposits()).toEqual({
      Err: { Unauthorized: null },
    });
    bridge.actor.setPrincipal(nextPausePrincipal);
    expect(await (bridge.actor as any).pause_new_deposits()).toHaveProperty("Ok");

    const [controller] = await pic!.getControllers(bridge.canisterId);
    if (controller === undefined) throw new Error("bridge controller is missing");
    await pic!.upgradeCanister({
      canisterId: bridge.canisterId,
      wasm: readFileSync(bridgeWasm),
      arg: IDL.encode([], []),
      sender: controller,
    });
    bridge.actor.setPrincipal(runtimePrincipal);
    const reopened: any = await (bridge.actor as any).get_public_config();
    expect(reopened.pause_principal.toText()).toBe(nextPausePrincipal.toText());
    expect(reopened.fee_recipient.owner.toText()).toBe(nextFeeRecipient.toText());
  });

  it("quotes the service fee only after the ledger pull", async () => {
    const { evm, bridge } = await setup();
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    expect(result).toHaveProperty("Ok");
    await (evm.actor as any).set_service_fee(7n);
    await mintAuthorizedDeposit(bridge, evm, result.Ok.deposit_id);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(stored[0].quote[0].service_fee).toBe(7n);
    expect(stored[0].quote[0].net_amount).toBe(19_993n);
    expect(phaseName(stored[0].state)).toBe("Minted");
  });

  it("reserves Mint capacity at quote and rejects a later window overflow before Ledger pull", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_mint_window(0n, 30_000n, 0n, 100n, 1n);
    const first: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    expect(first).toHaveProperty("Ok");
    await awaitMintAuthorization(bridge, first.Ok.deposit_id);
    await (evm.actor as any).set_mint_window(19_993n, 30_000n, 0n, 100n, 1n);
    const second: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_003n, max_service_fee: 10n });
    expect(second).toEqual({ Err: { Rejected: "MintWindowLimitExceeded" } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("does not make Deposit admission or Mint Authorization depend on the Mint Signer ETH balance", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_eth_balance(0n);
    const accepted: any = await (bridge.actor as any).request_deposit({
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 20_000n,
      max_service_fee: 10n,
    });
    expect(accepted).toHaveProperty("Ok.state.EscrowedUnquoted");
    await awaitMintAuthorization(bridge, accepted.Ok.deposit_id);
    const record: any = await (bridge.actor as any).get_deposit(accepted.Ok.deposit_id);
    expect(phaseName(record[0].state)).toBe("AuthorizationAvailable");
    expect(record[0].refund).toEqual([]);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
  });

  it("holds an ambiguous refund and resolves only the same refund identity", async () => {
    const { ledger, evm, bridge } = await setup();
    const accepted: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    await awaitMintAuthorization(bridge, accepted.Ok.deposit_id);
    await (ledger.actor as any).set_refund_ledger_mode([{ Trap: null }]);
    expect((await expireUnusedAuthorization(bridge, evm, accepted.Ok.deposit_id)).result).toHaveProperty("Ok.Stopped.reason.LedgerAmbiguous");
    const held: any = await (bridge.actor as any).get_deposit(accepted.Ok.deposit_id);
    expect(phaseName(held[0].state)).toBe("RefundReconciliationHold");
    expect(held[0].refund[0]).toMatchObject({ amount: 10_000n, ledger_fee: 10_000n, attempt_no: 0n, block_index: [] });
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(1);

    await (ledger.actor as any).set_refund_ledger_mode([{ Succeed: null }]);
    await advancePastAutomaticRetry();
    expect(await continueDeposit(bridge, accepted.Ok.deposit_id)).toHaveProperty("Ok.Complete");
    const refunded: any = await (bridge.actor as any).get_deposit(accepted.Ok.deposit_id);
    expect(phaseName(refunded[0].state)).toBe("Refunded");
    expect(refunded[0].refund[0]).toMatchObject({ amount: 10_000n, ledger_fee: 10_000n, attempt_no: 0n });
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(2);
  });

  it("creates a new fixed-payload refund attempt only after complete absence proof", async () => {
    const { ledger, index, evm, bridge } = await setup();
    const accepted: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    await awaitMintAuthorization(bridge, accepted.Ok.deposit_id);
    await (ledger.actor as any).set_refund_ledger_mode([{ Trap: null }]);
    expect((await expireUnusedAuthorization(bridge, evm, accepted.Ok.deposit_id)).result).toHaveProperty("Ok.Stopped.reason.LedgerAmbiguous");
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(1);

    await (index.actor as any).set_index_synced_blocks([1n]);
    await pic!.advanceTime(24 * 60 * 60 * 1_000 + 1);
    await (ledger.actor as any).set_refund_ledger_mode([{ Succeed: null }]);
    expect(await continueDeposit(bridge, accepted.Ok.deposit_id)).toHaveProperty("Ok.Complete");
    const refunded: any = await (bridge.actor as any).get_deposit(accepted.Ok.deposit_id);
    expect(phaseName(refunded[0].state)).toBe("Refunded");
    expect(refunded[0].refund[0]).toMatchObject({ amount: 10_000n, ledger_fee: 10_000n, attempt_no: 1n });
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(2);
  });

  it("stops a refund BadFee without changing the fixed refund payload", async () => {
    const { ledger, evm, bridge } = await setup();
    const accepted: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    await awaitMintAuthorization(bridge, accepted.Ok.deposit_id);
    await (ledger.actor as any).set_ledger_fee(12_000n);
    await (ledger.actor as any).set_refund_ledger_mode([{ BadFee: null }]);
    expect((await expireUnusedAuthorization(bridge, evm, accepted.Ok.deposit_id)).result).toHaveProperty("Ok.Stopped");
    const stopped: any = await (bridge.actor as any).get_deposit(accepted.Ok.deposit_id);
    expect(phaseName(stopped[0].state)).toBe("RefundPending");
    expect(stopped[0].refund[0]).toMatchObject({ amount: 10_000n, ledger_fee: 10_000n, attempt_no: 0n });
    expect(stopped[0].last_settlement_stop_reason[0]).toContain("BadFee");

    await (ledger.actor as any).set_refund_ledger_mode([{ Succeed: null }]);
    expect(await continueDeposit(bridge, accepted.Ok.deposit_id)).toHaveProperty("Ok.Complete");
    const refunded: any = await (bridge.actor as any).get_deposit(accepted.Ok.deposit_id);
    expect(refunded[0].refund[0]).toMatchObject({ amount: 10_000n, ledger_fee: 10_000n, attempt_no: 0n });
  });

  it("treats a full expired Mint window as having zero effective consumption", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_mint_window(30_000n, 30_000n, 0n, 10n, 11n);
    const accepted: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    expect(accepted).toHaveProperty("Ok");
    await awaitMintAuthorization(bridge, accepted.Ok.deposit_id);
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
  });

  it("refreshes at most one stale Mint snapshot per request and fails closed", async () => {
    const { ledger, evm, bridge } = await setup();
    const seed: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    await mintAuthorizedDeposit(bridge, evm, seed.Ok.deposit_id);
    expect(phaseName((await (bridge.actor as any).get_deposit(seed.Ok.deposit_id))[0].state)).toBe("Minted");

    await pic!.advanceTime(61_000);
    await (evm.actor as any).set_finalized_block_sequence([98n, 100n]);
    const stale: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    expect(stale).toEqual({ Err: { BaseObservationUnavailable: null } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
    await pic!.advanceTime(61_000);
    const recovered: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    expect(recovered).toHaveProperty("Ok");
    await awaitMintAuthorization(bridge, recovered.Ok.deposit_id);
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(2);

    await pic!.advanceTime(61_000);
    await (evm.actor as any).set_finalized_block_sequence([98n, 98n, 98n, 98n, 98n]);
    const unavailable: any = await (bridge.actor as any).request_deposit({ owner_sequence: 2n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    expect(unavailable).toEqual({ Err: { BaseObservationUnavailable: null } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(2);
  });

  it("reuses a finalized Base Mint snapshot within the admission TTL", async () => {
    const { evm, bridge } = await setup();
    const before = await (evm.actor as any).eth_call_count();
    const first: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    const afterFirst = await (evm.actor as any).eth_call_count();
    expect(afterFirst).toBeGreaterThan(before);
    await continueDeposit(bridge, first.Ok.deposit_id);
    const afterQuote = await (evm.actor as any).eth_call_count();
    const second: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    const afterSecond = await (evm.actor as any).eth_call_count();
    expect(afterQuote).toBeGreaterThan(afterFirst);
    expect(afterSecond).toBe(afterQuote + 1n);
    expect(second).toHaveProperty("Ok.state.EscrowedUnquoted");
  });

  it("refunds after pulling when the finalized Base snapshot is paused", async () => {
    const { ledger, evm, bridge } = await setup();
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    expect(result).toHaveProperty("Ok.state.EscrowedUnquoted");
    await (evm.actor as any).set_deposit_mints_paused(true);
    expect(await continueDeposit(bridge, result.Ok.deposit_id)).toHaveProperty("Ok.Complete");
    const record: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(record[0].refund[0].reason).toEqual({ BasePaused: null });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(2);
  });

  it("keeps funds escrowed without refund when the Base signer observation disagrees", async () => {
    const { ledger, evm, bridge } = await setup();
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    expect(result).toHaveProperty("Ok.state.EscrowedUnquoted");
    expect(await (evm.actor as any).set_bridge_signer(new Uint8Array(20).fill(0xaa))).toHaveProperty("Ok");
    expect(await continueDeposit(bridge, result.Ok.deposit_id)).toHaveProperty("Ok.Stopped");
    const record: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(phaseName(record[0].state)).toBe("EscrowedUnquoted");
    expect(record[0].refund).toEqual([]);
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("rate-limits new deposit admissions while preserving idempotent retries", async () => {
    const { bridge } = await setup();
    const request = (tag: number) => ({ owner_sequence: BigInt(tag - 72), base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
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

  it("does not let unfunded principals consume formal deposit admission", async () => {
    const { ledger, bridge, runtimePrincipal } = await setup();
    await (ledger.actor as any).set_ledger_mode({ InsufficientAllowance: { allowance: 0n } });
    const failedOwners: Principal[] = [];
    for (let index = 0; index < 101; index += 1) {
      const owner = Principal.selfAuthenticating(new Uint8Array(32).fill(index + 1));
      failedOwners.push(owner);
      bridge.actor.setPrincipal(owner);
      const rejected: any = await (bridge.actor as any).request_deposit({
        owner_sequence: 0n,
        base_recipient: new Uint8Array(20).fill(4),
        from_subaccount: [],
        gross_amount: 20_000n,
        max_service_fee: 10n,
      });
      expect(rejected).toHaveProperty("Err.FundingRejected.InsufficientAllowance.allowance", 0n);
    }
    expect(await (bridge.actor as any).get_next_deposit_sequence(failedOwners[0])).toBe(0n);
    expect((await (bridge.actor as any).list_deposit_ids({
      owner: failedOwners[0],
      before_cursor: [],
      limit: 20,
    })).Ok.deposit_ids).toEqual([]);

    bridge.actor.setPrincipal(runtimePrincipal);
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    const funded: any = await (bridge.actor as any).request_deposit({
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 20_000n,
      max_service_fee: 10n,
    });
    expect(funded).toHaveProperty("Ok.state.EscrowedUnquoted");
    expect((await (bridge.actor as any).get_bridge_status()).counts.deposits).toBe(1n);
  });

  it("rejects locally paused admissions before pull while preserving accepted replay", async () => {
    const { ledger, bridge, init, runtimePrincipal } = await setup();
    const args = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n };
    bridge.actor.setPrincipal(init.pause_principal);
    await (bridge.actor as any).pause_new_deposits();
    bridge.actor.setPrincipal(runtimePrincipal);
    expect(await (bridge.actor as any).request_deposit(args)).toEqual({ Err: { DepositsPaused: null } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);

    await (bridge.actor as any).resume_new_deposits();
    const accepted: any = await (bridge.actor as any).request_deposit(args);
    expect(accepted).toHaveProperty("Ok");
    bridge.actor.setPrincipal(init.pause_principal);
    await (bridge.actor as any).pause_new_deposits();
    bridge.actor.setPrincipal(runtimePrincipal);
    const replay: any = await (bridge.actor as any).request_deposit(args);
    expect(replay).toHaveProperty("Ok");
    expect(Array.from(replay.Ok.deposit_id)).toEqual(Array.from(accepted.Ok.deposit_id));
  });

  it("accepts a finalized committed withdrawal and pays ICP without another Base transaction", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(6);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100_000n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
    const ingested: any = await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    expect(ingested).toHaveProperty("Ok.Ingested");
    expect(ingested.Ok.Ingested).not.toHaveProperty("settlement");
    expect(Array.from(ingested.Ok.Ingested.withdrawal_id)).toEqual(Array.from(id));
    expect(phaseName((await (bridge.actor as any).get_withdrawal(id))[0].state)).toBe("ReleasePending");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(await continueWithdrawal(bridge, id)).toHaveProperty("Ok.Complete");
    expect(phaseName((await (bridge.actor as any).get_withdrawal(id))[0].state)).toBe("Paid");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    await (ledger.actor as any).set_ledger_fee_available(false);
    await (evm.actor as any).set_observed_transaction(new Uint8Array(32).fill(9), new Uint8Array(20).fill(1), new Uint8Array(20).fill(0x22), 99n);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100_000n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
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
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100_000n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
    await (evm.actor as any).set_finalized_block_sequence([98n]);
    const premature: any = await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    expect(premature).toHaveProperty("Err.TransactionNotConfirmed");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    await (evm.actor as any).set_finalized_block_sequence([100n]);
    const notified: any = await notifyFixtureWithdrawal(bridge);
    expect(notified.Ok.Ingested).not.toHaveProperty("settlement");
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
      await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100_000n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
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
      amount: 100_000n,
      max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n,
    }]);

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) }))
      .toHaveProperty("Ok.Ingested");
    expect(Array.from(await (evm.actor as any).pinned_eth_call_block_numbers())).toEqual([99n, 100n, 100n]);
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
      amount: 100_000n,
      max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n,
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
      amount: 100_000n,
      max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n,
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
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100_000n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
    await (evm.actor as any).set_withdrawal_status(0);

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toEqual({ Err: { BaseStateMismatch: null } });
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  });

  it("rejects signer rotation between the receipt and finalized Base state read before Ledger", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xa2);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100_000n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
    expect(await (evm.actor as any).set_bridge_signer(new Uint8Array(20).fill(0xaa))).toHaveProperty("Ok");

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toEqual({ Err: { BridgeSignerMismatch: null } });
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  });

  it("rejects non-confirmed and wrong-owner notifications and ingests one concurrent replay", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(86);
    await (evm.actor as any).set_withdrawal([{ id, owner: Principal.selfAuthenticating(new Uint8Array(32).fill(8)).toUint8Array(), subaccount: new Uint8Array(32), amount: 100_000n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
    bridge.actor.setPrincipal(Principal.selfAuthenticating(new Uint8Array(32).fill(9)));
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Err.OwnerMismatch");
    bridge.actor.setPrincipal(runtimePrincipal);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100_000n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
    await (evm.actor as any).set_observed_transaction(new Uint8Array(32).fill(9), new Uint8Array(20).fill(1), new Uint8Array(20).fill(0x22), 101n);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Err.TransactionNotConfirmed");
    await (evm.actor as any).set_observed_transaction(new Uint8Array(32).fill(9), new Uint8Array(20).fill(1), new Uint8Array(20).fill(0x22), 99n);
    const deferred = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    deferred.setPrincipal(runtimePrincipal);
    const first = await deferred.notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    const second = await deferred.notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    const results: any[] = await Promise.all([first(), second()]);
    expect(results.filter((result) => "Ok" in result && "Ingested" in result.Ok)).toHaveLength(1);
    expect(
      results.filter(
        (result) =>
          ("Err" in result && "Busy" in result.Err)
          || ("Ok" in result && "Duplicate" in result.Ok),
      ),
    ).toHaveLength(1);
    expect(results.some((result) => "Err" in result && "RateLimited" in result.Err)).toBe(false);
    expect((await (bridge.actor as any).get_bridge_status()).counts.withdrawals).toBe(1n);
    expect(await (bridge.actor as any).notify_withdrawal({
      transaction_hash: new Uint8Array(32).fill(9),
    })).toHaveProperty("Ok.Duplicate");
  });

  it("returns the canonical duplicate for a known notification hash without re-reading RPC", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(89);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100_000n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Ok.Ingested");
    const callsAfterIngest = await (evm.actor as any).receipt_call_count();
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100_001n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Ok.Duplicate");
    expect(await (evm.actor as any).receipt_call_count()).toBe(callsAfterIngest);
  });

  it("persistently rate limits repeated unknown notification hashes before further RPC", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(88);
    await (evm.actor as any).set_observed_transaction(new Uint8Array(32).fill(9), new Uint8Array(20).fill(1), new Uint8Array(20).fill(0x22), 101n);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100_000n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
    const callsBefore = await (evm.actor as any).receipt_call_count();
    for (let attempt = 0; attempt < 3; attempt += 1) {
      expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Err.TransactionNotConfirmed");
    }
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Err.RateLimited");
    expect(await (evm.actor as any).receipt_call_count()).toBe(callsBefore + 3n);
  });

  it("uses the fixed fee without querying Ledger fee availability", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(87);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100_000n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
    await (ledger.actor as any).set_ledger_fee_available(false);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Ok.Ingested");
    const paid: any = await (bridge.actor as any).get_withdrawal(id);
    expect(paid[0].ledger_fee).toBe(10_000n);
  });

  it("fails closed when the charged service fee is below the fixed Ledger fee", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xb7);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 10_000n, max_service_fee: 9_999n, charged_service_fee: 9_999n, amount_out: 1n }]);

    const guarded: any = await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    expect(guarded).toEqual({ Err: { LedgerFeeExceedsServiceFee: { ledger_fee: 10_000n, charged_service_fee: 9_999n } } });
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    const blocked: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(blocked[0].state)).toBe("Observed");
    expect(blocked[0].last_settlement_stop_reason).toEqual(["LedgerFeeExceedsServiceFee"]);
    expect((await (bridge.actor as any).get_bridge_status()).withdrawal_fee_guard_active).toBe(true);
  });

  it("continues an ambiguous Withdrawal release from reconciled Hold to Paid", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    const id = new Uint8Array(32).fill(46);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100_000n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
    await notifyFixtureWithdrawal(bridge);
    const held: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(held[0].state)).toBe("ReconciliationHold");
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    await advancePastAutomaticRetry();
    expect(await continueWithdrawal(bridge, id)).toHaveProperty("Ok.Complete");
    const released: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(released[0].state)).toBe("Paid");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("stops an unexpected BadFee without changing the Withdrawal transfer identity", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xb1);
    await (ledger.actor as any).set_ledger_fee(1n);
    await (ledger.actor as any).set_ledger_mode({ BadFee: null });
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100_000n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
    await notifyFixtureWithdrawal(bridge);

    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    const stopped: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(stopped[0].state)).toBe("ReleasePending");
    expect(stopped[0].ledger_fee).toBe(10_000n);
    expect(stopped[0].last_settlement_stop_reason[0]).toContain("BadFee");

    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    expect(await continueWithdrawal(bridge, id)).toHaveProperty("Ok.Complete");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(2n);
    const paid: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(paid[0].state)).toBe("Paid");
    expect(paid[0].ledger_fee).toBe(10_000n);
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });


  it("does not reprice or cancel after an ambiguous Ledger release", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xb5);
    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 100_000n, max_service_fee: 10_000n, charged_service_fee: 10_000n, amount_out: 90_000n }]);
    await notifyFixtureWithdrawal(bridge);
    expect(phaseName((await (bridge.actor as any).get_withdrawal(id))[0].state)).toBe("ReconciliationHold");

    await (ledger.actor as any).set_ledger_fee(2n);
    await (ledger.actor as any).set_ledger_mode({ BadFee: null });
    await advancePastAutomaticRetry();
    const retry: any = await continueWithdrawal(bridge, id);
    expect(retry).toHaveProperty("Ok.Stopped.reason.LedgerRejected");
    expect(phaseName((await (bridge.actor as any).get_withdrawal(id))[0].state)).toBe("ReconciliationHold");
  });


  it("continues an ambiguous deposit from reconciled Hold to a signed Mint Authorization", async () => {
    const { ledger, evm, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n });
    expect(phaseName(result.Ok.state)).toBe("FundingReconciliationHold");
    expect(phaseName((await (bridge.actor as any).get_deposit(result.Ok.deposit_id))[0].state)).toBe("FundingReconciliationHold");
    const before: any = await (bridge.actor as any).get_bridge_status();
    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    const after: any = await (bridge.actor as any).get_bridge_status();
    expect(after.counts.reconciliation_holds).toBe(before.counts.reconciliation_holds);
    expect(after.counts.reconciliation_holds).toBe(1n);
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    await advancePastAutomaticRetry();
    await mintAuthorizedDeposit(bridge, evm, result.Ok.deposit_id);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("Minted");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("retains one retryable funding identity and retries only the same payload", async () => {
    const { ledger, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode({ TemporarilyUnavailable: null });

    const request = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n };
    const result: any = await (bridge.actor as any).request_deposit(request);

    expect(result).toHaveProperty("Err.FundingUnavailable.retry_after_seconds", 30n);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    expect(await (bridge.actor as any).request_deposit(request)).toHaveProperty("Err.FundingUnavailable");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    await advanceClock(31);
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    expect(await (bridge.actor as any).request_deposit(request)).toHaveProperty("Ok.state.EscrowedUnquoted");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(2n);
  });

  it("automatically refunds an unused Authorization only after the finalized deadline", async () => {
    const { ledger, evm, bridge } = await setup();
    const result: any = await requestDefaultDeposit(bridge);
    await assertAuthorizationRefunded(bridge, evm, result.Ok.deposit_id);
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(2);
  });

  it("fails closed and pauses new deposits when processed is true but exact Mint evidence is missing", async () => {
    const { evm, bridge } = await setup();
    const result: any = await requestDefaultDeposit(bridge);
    const authorization = await awaitMintAuthorization(bridge, result.Ok.deposit_id);
    await evm.actor.set_processed_deposit(true);
    await evm.actor.set_mint_log([]);
    await evm.actor.set_block_timestamp(authorization.deadline + 1n);
    expect(await continueDeposit(bridge, result.Ok.deposit_id)).toHaveProperty("Ok.Stopped.reason.BaseStateMismatch");
    const stored: any = await bridge.actor.get_deposit(result.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("ExpiryReconciliation");
    expect(stored[0].refund).toEqual([]);
    expect((await bridge.actor.get_bridge_status()).deposits_paused).toBe(true);
  });

  it("does not refund when finalized Base RPC observations disagree", async () => {
    const { evm, bridge } = await setup();
    const result: any = await requestDefaultDeposit(bridge);
    const authorization = await awaitMintAuthorization(bridge, result.Ok.deposit_id);
    await evm.actor.set_block_timestamp(authorization.deadline + 1n);
    await evm.actor.set_block_mode({ FinalizedInconsistent: null });
    expect(await continueDeposit(bridge, result.Ok.deposit_id)).toHaveProperty("Ok.Stopped.reason.RpcInconsistent");
    const stored: any = await bridge.actor.get_deposit(result.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("AuthorizationAvailable");
    expect(stored[0].refund).toEqual([]);
  });

  it("keeps invalid notification traffic isolated from valid withdrawal ingestion", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    for (let index = 0; index < 61; index += 1) {
      bridge.actor.setPrincipal(
        Principal.selfAuthenticating(new Uint8Array(32).fill(Math.floor(index / 6) + 200)),
      );
      const rejected: any = await (bridge.actor as any).notify_withdrawal({
        transaction_hash: new Uint8Array(32).fill(index + 100),
      });
      expect(rejected).toHaveProperty("Err");
      expect(rejected.Err).not.toHaveProperty("RateLimited");
    }

    bridge.actor.setPrincipal(runtimePrincipal);
    const id = new Uint8Array(32).fill(0x91);
    await (evm.actor as any).set_withdrawal([{
      id,
      owner: runtimePrincipal.toUint8Array(),
      subaccount: new Uint8Array(32),
      amount: 100_000n,
      max_service_fee: 10_000n,
      charged_service_fee: 10_000n,
      amount_out: 90_000n,
    }]);
    expect(await notifyFixtureWithdrawal(bridge)).toHaveProperty("Ok.Ingested");
    expect(await (bridge.actor as any).get_withdrawal(id)).toHaveLength(1);
  });

  it("keeps retryable funding outside the public settlement quota", async () => {
    const { ledger, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode({ TemporarilyUnavailable: null });
    const request = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n };
    const result: any = await (bridge.actor as any).request_deposit(request);
    expect(result).toHaveProperty("Err.FundingUnavailable");
    for (let attempt = 0; attempt < 3; attempt += 1) {
      await advanceClock(31);
      expect(await (bridge.actor as any).request_deposit(request)).toHaveProperty("Err.FundingUnavailable");
    }
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(4n);
  });

  it("does not refund early after an epoch change and still requires finalized expiry evidence", async () => {
    const { evm, bridge } = await setup();
    const result: any = await requestDefaultDeposit(bridge);
    const authorization = await awaitMintAuthorization(bridge, result.Ok.deposit_id);
    await evm.actor.set_mint_authorization_epoch(authorization.authorization_epoch + 1n);
    await evm.actor.set_block_timestamp(authorization.deadline);
    expect(await continueDeposit(bridge, result.Ok.deposit_id)).toHaveProperty("Ok.Deferred");
    let stored: any = await bridge.actor.get_deposit(result.Ok.deposit_id);
    expect(stored[0].refund).toEqual([]);

    await evm.actor.set_block_timestamp(authorization.deadline + 1n);
    expect(await continueDeposit(bridge, result.Ok.deposit_id)).toHaveProperty("Ok.Complete");
    stored = await bridge.actor.get_deposit(result.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("Refunded");
  });



  it("pauses only new deposits and allows Governance to resume them", async () => {
    const { evm, bridge, init, runtimePrincipal } = await setup();
    await (evm.actor as any).set_max_service_fee(30_000n);
    await (evm.actor as any).set_service_fee(30_000n);
    bridge.actor.setPrincipal(init.pause_principal);
    expect(await (bridge.actor as any).pause_new_deposits()).toHaveProperty("Ok");
    bridge.actor.setPrincipal(runtimePrincipal);
    const args = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 40_000n, max_service_fee: 30_000n };
    expect(await (bridge.actor as any).request_deposit(args)).toEqual({ Err: { DepositsPaused: null } });
    expect(await (bridge.actor as any).resume_new_deposits()).toHaveProperty("Ok");
    const deposit: any = await (bridge.actor as any).request_deposit(args);
    expect(deposit).toHaveProperty("Ok");
    const audit: any = await (bridge.actor as any).get_audit_events(0n, 100);
    expect(audit.Ok.events.length).toBeGreaterThanOrEqual(2);
    expect(await (bridge.actor as any).request_fee_payout(1n)).toEqual({ Err: { InsufficientFeeReserve: null } });
    await mintAuthorizedDeposit(bridge, evm, deposit.Ok.deposit_id);
    expect(phaseName((await (bridge.actor as any).get_deposit(deposit.Ok.deposit_id))[0].state)).toBe("Minted");
    expect(await (bridge.actor as any).request_fee_payout(1n)).toHaveProperty("Ok");
  });

  it("installs with new deposits paused until Governance activates them", async () => {
    const { bridge } = await setup(false);
    expect((await (bridge.actor as any).get_bridge_status()).deposits_paused).toBe(true);
    const args = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n };
    expect(await (bridge.actor as any).request_deposit(args)).toEqual({ Err: { DepositsPaused: null } });
  });

  it("keeps Mint gas outside Deposit reserve admission", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_eth_balance(0n);
    const args = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n };
    const result: any = await (bridge.actor as any).request_deposit(args);
    expect(result).toHaveProperty("Ok.state.EscrowedUnquoted");
    await awaitMintAuthorization(bridge, result.Ok.deposit_id);
    expect(phaseName((await (bridge.actor as any).get_deposit(result.Ok.deposit_id))[0].state)).toBe("AuthorizationAvailable");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("rejects a definitive Ledger pull failure without creating formal deposit artifacts", async () => {
    const { ledger, bridge, runtimePrincipal } = await setup();
    const failed = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 20_000n, max_service_fee: 10n };
    await (ledger.actor as any).set_ledger_mode({ BadFee: null });
    const result: any = await (bridge.actor as any).request_deposit(failed);
    expect(result).toHaveProperty("Err.FundingRejected.BadFee");
    let status: any = await (bridge.actor as any).get_bridge_status();
    expect(status.counts.reserved_deposit_mint_amount).toBe(0n);
    expect(await (bridge.actor as any).get_next_deposit_sequence(runtimePrincipal)).toBe(0n);
    expect((await (bridge.actor as any).list_deposit_ids({ owner: runtimePrincipal, before_cursor: [], limit: 20 })).Ok.deposit_ids).toEqual([]);

    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    await advanceTimeWithoutSettlement(2);
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);
    expect(await (bridge.actor as any).request_deposit(failed)).toHaveProperty("Ok.state.EscrowedUnquoted");
    status = await (bridge.actor as any).get_bridge_status();
    expect(status.counts.reserved_deposit_mint_amount).toBe(0n);
  });

  it.each([
    ["InsufficientAllowance", { InsufficientAllowance: { allowance: 0n } }],
    ["InsufficientFunds", { InsufficientFunds: { balance: 0n } }],
  ])("rejects a definitive %s pull without creating a formal record", async (label, mode) => {
    const { ledger, bridge, runtimePrincipal } = await setup();
    await (ledger.actor as any).set_ledger_mode(mode);
    const result: any = await (bridge.actor as any).request_deposit({
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 20_000n,
      max_service_fee: 10n,
    });
    expect(result).toHaveProperty(`Err.FundingRejected.${label}`);
    expect(await (bridge.actor as any).get_next_deposit_sequence(runtimePrincipal)).toBe(0n);
    expect((await (bridge.actor as any).list_deposit_ids({ owner: runtimePrincipal, before_cursor: [], limit: 20 })).Ok.deposit_ids).toEqual([]);
    expect(await (ledger.actor as any).ledger_transactions()).toEqual([]);
  });

  it("serves configured transaction prefixes through the ICRC archive callback", async () => {
    const { ledger, bridge } = await setup();
    const deposited: any = await (bridge.actor as any).request_deposit({
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 20_000n,
      max_service_fee: 10n,
    });
    await continueDeposit(bridge, deposited.Ok.deposit_id);
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
      gross_amount: 20_000n,
      max_service_fee: 10n,
    });
    expect(first).toHaveProperty("Ok");
    await continueDeposit(bridge, first.Ok.deposit_id);
    expect(await (ledger.actor as any).ledger_transactions()).toHaveLength(1);

    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    const ambiguous: any = await (bridge.actor as any).request_deposit({
      owner_sequence: 1n,
      base_recipient: new Uint8Array(20).fill(5),
      from_subaccount: [],
      gross_amount: 20_000n,
      max_service_fee: 10n,
    });
    expect(await continueDeposit(bridge, ambiguous.Ok.deposit_id)).toHaveProperty("Ok.Stopped");
    expect(phaseName((await (bridge.actor as any).get_deposit(ambiguous.Ok.deposit_id))[0].state)).toBe("FundingReconciliationHold");

    await (index.actor as any).set_index_synced_blocks([0n]);
    await pic!.advanceTime(24 * 60 * 60 * 1_000 + 1);
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    const reconciliation = await continueDeposit(bridge, ambiguous.Ok.deposit_id);
    expect(
      reconciliation,
    ).toEqual(
      expect.objectContaining(
        "Ok" in reconciliation
          ? { Ok: expect.objectContaining({ ReconciliationProgress: expect.anything() }) }
          : { Err: expect.objectContaining({ AutomaticProgressPending: expect.anything() }) },
      ),
    );
    expect(phaseName((await (bridge.actor as any).get_deposit(ambiguous.Ok.deposit_id))[0].state)).toBe(
      "FundingReconciliationHold",
    );
    expect(await (ledger.actor as any).ledger_transactions()).toHaveLength(1);

    await (index.actor as any).set_index_synced_blocks([1n]);
    await advancePastAutomaticRetry();
    expect(await continueDeposit(bridge, ambiguous.Ok.deposit_id)).toHaveProperty(
      "Ok.Complete",
    );
    expect(phaseName((await (bridge.actor as any).get_deposit(ambiguous.Ok.deposit_id))[0].state)).toBe(
      "Cancelled",
    );
    expect(await (ledger.actor as any).ledger_transactions()).toHaveLength(1);
  });

  it("fails a retryable fee payout without trapping its reserve", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_max_service_fee(30_000n);
    await (evm.actor as any).set_service_fee(30_000n);
    const deposit: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 40_000n, max_service_fee: 30_000n });
    expect(deposit).toHaveProperty("Ok");
    await mintAuthorizedDeposit(bridge, evm, deposit.Ok.deposit_id);
    expect(phaseName((await (bridge.actor as any).get_deposit(deposit.Ok.deposit_id))[0].state)).toBe("Minted");
    await (ledger.actor as any).set_ledger_mode({ TemporarilyUnavailable: null });
    const callsBeforeRequest = await (ledger.actor as any).ledger_transfer_calls();
    const failed: any = await (bridge.actor as any).request_fee_payout(1n);
    expect(failed).toHaveProperty("Ok");
    expect(failed.Ok.state).toEqual({ Pending: null });
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(callsBeforeRequest);
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
    expect(before.schema_version).toBe(25);
    expect(before.counts.withdrawals).toBe(10_000n);
    expect(before.counts.retained_audit_events).toBe(10_000n);
    expect(before.settlement_scheduler.scheduled).toBe(10_000n);
    expect(before.unpaid_withdrawal_count).toBe(10_000n);
    expect(before.unpaid_withdrawal_amount_out).toBe(900_000n);

    const firstId = createHash("sha256")
      .update("KINIC_BRIDGE_STORAGE_SEED_V2")
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
    expect(after.schema_version).toBe(25);
    expect(after.counts).toEqual(before.counts);
    expect(after.settlement_scheduler.scheduled).toBe(10_000n);
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
