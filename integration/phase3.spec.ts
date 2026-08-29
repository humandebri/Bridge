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
const bridgeWasm = resolve(root, "target/test-deployment/staging/bridge_canister.wasm");
const mockWasm = resolve(root, "target/wasm32-unknown-unknown/release/mock_external.wasm");
const testLedgerFee = 10_000n;

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

  async function setup(
    activate = true,
    initOverrides: Record<string, unknown> = {},
    wasmPath = bridgeWasm,
  ) {
    const mockBytes = readFileSync(mockWasm);
    const subnet = await pic!.getFiduciarySubnet();
    if (subnet === undefined) throw new Error("Fiduciary subnet was not created");
    const installMock = (ledgerId: Principal): Promise<any> => pic!.setupCanister({ idlFactory: mockIdl, wasm: mockBytes, arg: IDL.encode([mockInit], [{ ledger_id: ledgerId }]), cycles: 50_000_000_000_000n, targetSubnetId: subnet.id });
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
    const confirmationRelayerPrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(8));
    const feeRecipientPrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(55));
    const init = { ledger_canister_id: ledger.canisterId, index_canister_id: index.canisterId, evm_rpc_canister_id: evm.canisterId, custom_evm_rpc_urls: [], base_chain_id: 8453n, bridge_contract: new Uint8Array(20).fill(1), expected_bridge_runtime_sha256: new Uint8Array(createHash("sha256").update(new Uint8Array([0x60, 0x00])).digest()), timelock_contract: new Uint8Array(20).fill(2), expected_timelock_minimum_delay_seconds: 300n, expected_bsns_runtime_sha256: new Uint8Array(createHash("sha256").update(new Uint8Array([0x60, 0x02])).digest()), expected_bsns_decimals: 8, expected_minimum_service_fee: 1n, deployment_instance_id: new Uint8Array(32).fill(3), minimum_withdrawal_id: new Uint8Array([...new Uint8Array(31), 1]), ecdsa_key_name: "key_1", ecdsa_derivation_path: [], governance_ecdsa_derivation_path: [new TextEncoder().encode("governance-operator")], deposit_rate_limit_window_seconds: 60n, deposit_rate_limit_global: 30, deposit_rate_limit_per_principal: 3, notification_rate_limit_window_seconds: 600n, notification_rate_limit_global: 60, notification_ingestion_rate_limit_global: 30, settlement_rate_limit_window_seconds: 3_600n, settlement_rate_limit_global: 60, settlement_rate_limit_per_principal: 30, settlement_rate_limit_per_record: 3, settlement_retry_interval_seconds: 60n, governance_evm_fee: { gas_limit_ceiling: 500_000n, max_fee_per_gas_ceiling: 200_000_000_000n, max_priority_fee_per_gas_ceiling: 10_000_000_000n, l1_fee_per_transaction_ceiling_wei: 10_000_000_000_000_000n, quote_validity_seconds: 90n, gas_limit_multiplier_bps: 13_000, base_fee_multiplier_bps: 60_000, l1_fee_multiplier_bps: 15_000 }, governance_replacement: { max_replacements: 3, fee_bump_bps: 1_250 }, cycles_floor: 1n, settlement_cycle_ceiling: 1n, governance_principal: runtimePrincipal, pause_principal: Principal.selfAuthenticating(new Uint8Array(32).fill(34)), confirmation_relayer_principal: confirmationRelayerPrincipal, fee_recipient: { owner: feeRecipientPrincipal, subaccount: [] } };
    Object.assign(init, initOverrides);
    const bridge: any = await pic!.setupCanister({ idlFactory: bridgeIdl, wasm: readFileSync(wasmPath), arg: IDL.encode([bridgeInit], [init]), cycles: 500_000_000_000_000n, targetSubnetId: subnet.id });
    expect(await (bridge.actor as any).initialize_public_config()).toHaveProperty("Ok");
    bridge.actor.setPrincipal(runtimePrincipal);
    const configuredSigner: any = await (evm.actor as any).set_bridge_signer_for_canister(bridge.canisterId, init.ecdsa_key_name);
    if (!("Ok" in configuredSigner)) throw new Error(`failed to configure mock bridge signer: ${configuredSigner.Err}`);
    const configuredRoleSigners: any = await (evm.actor as any).set_deployment_role_signers_for_canister(bridge.canisterId, init.ecdsa_key_name);
    if (!("Ok" in configuredRoleSigners)) throw new Error(`failed to configure mock deployment role signers: ${configuredRoleSigners.Err}`);
    // The canister rejects an empty eth_getCode result before admitting deposits.
    // A fixed non-empty mock runtime keeps the observed bridge identity deterministic.
    await (evm.actor as any).set_bridge_runtime_code(new Uint8Array([0x60, 0x00]));
    // Phase 3 keeps small 1/10 fee boundaries; bind its test-only immutable explicitly.
    await (evm.actor as any).set_minimum_service_fee(init.expected_minimum_service_fee);
    // Base and IC timestamps share Unix seconds, but Finalized may lag. Most tests
    // start aligned and dedicated cases move only Finalized behind IC issue time.
    await (evm.actor as any).set_block_timestamp(BigInt(Math.floor((await pic!.getTime()) / 1_000)));
    await (evm.actor as any).set_deposit_mints_paused(true);
    await (evm.actor as any).set_withdrawals_paused(true);
    const operationalConfig: any = await (bridge.actor as any).get_operational_config();
    expect(operationalConfig).toHaveProperty("Ok");
    expect(await (evm.actor as any).set_deployment_postconditions(
      init.timelock_contract,
      operationalConfig.Ok.governance_operator,
      new Uint8Array(20).fill(9),
      init.bridge_contract,
      new Uint8Array([0x60, 0x01]),
      new Uint8Array([0x60, 0x02]),
    )).toHaveProperty("Ok");
    expect(await (bridge.actor as any).seal_operational_config({
      governance_evm_fee: init.governance_evm_fee,
      cycles_floor: init.cycles_floor,
      settlement_cycle_ceiling: init.settlement_cycle_ceiling,
    })).toHaveProperty("Ok.lifecycle.OperationalConfigSealed");
    if (activate) await activateBridgeThroughGovernance(bridge, evm, runtimePrincipal);
    expect((await pic!.getCanisterSubnetId(bridge.canisterId))?.toText()).toBe(subnet.id.toText());
    return { ledger, index, evm, bridge, init, runtimePrincipal, confirmationRelayerPrincipal };
  }

  async function rejects_overlapping_confirmation_and_pause_roles_at_install() {
    const overlappingPrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(77));
    await expect(setup(false, {
      pause_principal: overlappingPrincipal,
      confirmation_relayer_principal: overlappingPrincipal,
    })).rejects.toThrow();
  }

  it(
    "rejects overlapping confirmation and pause roles at install",
    rejects_overlapping_confirmation_and_pause_roles_at_install,
  );

  async function activateBridgeThroughGovernance(bridge: any, evm: any, governance: Principal) {
    bridge.actor.setPrincipal(governance);
    await (evm.actor as any).set_deposit_mints_paused(true);
    await (evm.actor as any).set_withdrawals_paused(true);
    await (evm.actor as any).set_receipt_mode({ Confirmed: null });
    const auditBefore: any = await (bridge.actor as any).get_audit_events(0n, 100);
    const resumesBefore = auditBefore.Ok.events.filter(
      (event: any) => "DepositsResumed" in event.kind,
    ).length;

    const scheduled: any = await (bridge.actor as any).schedule_activation();
    expect(scheduled).toHaveProperty("Ok.kind.ScheduleActivation");
    expect(await (bridge.actor as any).confirm_base_governance_transaction({
      operation_id: scheduled.Ok.operation_id,
      transaction_hash: scheduled.Ok.transaction_hash,
    })).toHaveProperty("Ok.succeeded", true);

    await pic!.advanceTime(5 * 60_000 + 1);
    await pic!.tick(5);
    const executed: any = await (bridge.actor as any).execute_activation();
    expect(executed).toHaveProperty("Ok.kind.ExecuteActivation");
    await (evm.actor as any).set_deposit_mints_paused(false);
    await (evm.actor as any).set_withdrawals_paused(false);
    expect(await (bridge.actor as any).confirm_base_governance_transaction({
      operation_id: executed.Ok.operation_id,
      transaction_hash: executed.Ok.transaction_hash,
    })).toHaveProperty("Ok.succeeded", true);

    expect((await (bridge.actor as any).get_bridge_status()).deposits_paused).toBe(false);
    const auditAfter: any = await (bridge.actor as any).get_audit_events(0n, 100);
    expect(auditAfter.Ok.events.filter(
      (event: any) => "DepositsResumed" in event.kind,
    )).toHaveLength(resumesBefore + 1);
  }

  async function advanceTimeWithoutSettlement(rounds = 5) { for (let step = 0; step < rounds; step += 1) { await pic!.advanceTime(60_000); await pic!.tick(5); } }
  async function advanceClock(minutes: number) {
    await pic!.advanceTime(minutes * 60_000);
    await pic!.tick(30);
  }
  async function advancePastReconciliationDelay(minutes = 21) {
    await pic!.advanceTime(minutes * 60_000 + 1);
  }
  async function continueWithdrawal(bridge: any, withdrawalId: Uint8Array) {
    await pic!.advanceTime(6 * 60_000 + 1);
    return bridge.actor.continue_withdrawal(withdrawalId);
  }
  async function advanceDepositJobs(bridge: any, depositId: Uint8Array) {
    await advanceClock(6);
    return bridge.actor.get_deposit(depositId);
  }
  async function awaitMintAuthorization(bridge: any, depositId: Uint8Array) {
    const attempts: any[] = [];
    for (let attempt = 0; attempt < 8; attempt += 1) {
      const stored: any = await bridge.actor.get_deposit(depositId);
      const authorization = stored[0]?.mint_authorization?.[0];
      if (authorization?.signature?.[0] !== undefined) return authorization;
      attempts.push({
        state: stored[0]?.state,
        automatic_progress: stored[0]?.automatic_progress,
        last_settlement_stop_reason: stored[0]?.last_settlement_stop_reason,
        mint_authorization: stored[0]?.mint_authorization,
      });
      await pic!.advanceTime(60_001);
      await pic!.tick(30);
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
    await setExpiredBlockTimestamp(evm, authorization.deadline + 1n);
    let result = await (bridge.actor as any).request_deposit_refund(depositId);
    // The mock exposes one processed flag and one log list rather than a
    // deposit-keyed contract state. Do not leak one completed Mint into the
    // next deposit's preflight.
    await evm.actor.set_processed_deposit(false);
    await evm.actor.set_mint_log([]);
    if ("Err" in result && "NotClaimable" in result.Err) {
      let stored: any = await bridge.actor.get_deposit(depositId);
      if (stored.length === 1 && phaseName(stored[0].state) !== "Minted") {
        await advancePastSnapshotCache();
        result = await (bridge.actor as any).request_deposit_refund(depositId);
        stored = await bridge.actor.get_deposit(depositId);
      }
      if (stored.length === 1 && phaseName(stored[0].state) === "Minted") return { Ok: stored[0] };
    }
    return result;
  }
  async function advancePastSnapshotCache() {
    await pic!.advanceTime(60_001);
    await pic!.tick(1);
  }
  async function setExpiredBlockTimestamp(evm: any, timestamp: bigint) {
    await advancePastSnapshotCache();
    await evm.actor.set_block_timestamp(timestamp);
  }
  async function expireUnusedAuthorization(bridge: any, evm: any, depositId: Uint8Array, timestampOffset = 1n) {
    const authorization = await awaitMintAuthorization(bridge, depositId);
    await evm.actor.set_processed_deposit(false);
    await evm.actor.set_mint_log([]);
    await setExpiredBlockTimestamp(evm, authorization.deadline + timestampOffset);
    return {
      authorization,
      result: await (bridge.actor as any).request_deposit_refund(depositId),
    };
  }
  async function assertAuthorizationRefunded(bridge: any, evm: any, depositId: Uint8Array) {
    const expired = await expireUnusedAuthorization(bridge, evm, depositId);
    expect(expired.result).toHaveProperty("Ok.state.Refunded");
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
      gross_amount: 200_000n,
      max_service_fee: 10n,
    });
  }

  it("mints_when_the_finalized_Base_head_is_twenty_minutes_behind_IC_issue_time", async () => {
    const { evm, bridge } = await setup();
    const icTimestamp = BigInt(Math.floor((await pic!.getTime()) / 1_000));
    await (evm.actor as any).set_block_timestamp(icTimestamp - 20n * 60n);

    const result: any = await requestDefaultDeposit(bridge);
    expect(result).toHaveProperty("Ok.state.EscrowedUnquoted");
    await advanceDepositJobs(bridge, result.Ok.deposit_id);
    const authorization: any = await awaitMintAuthorization(bridge, result.Ok.deposit_id);

    expect(authorization.finalized_block_timestamp).toBeLessThan(authorization.issued_at_timestamp - 19n * 60n);
    expect(authorization.deadline).toBe(authorization.issued_at_timestamp + 600n);
    expect(authorization.signature).toHaveLength(1);
    expect(await mintAuthorizedDeposit(bridge, evm, result.Ok.deposit_id)).toHaveProperty("Ok.state.Minted");
  });
  async function notifyFixtureWithdrawal(bridge: any, transactionHash = new Uint8Array(32).fill(9)) {
    const result = await bridge.actor.notify_withdrawal({ transaction_hash: transactionHash });
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

  async function persists_one_idempotent_Deposit_through_ledger_pull_Mint_Authorization_and_finalized_exact_Mint_evidence() {
    const { bridge, ledger, evm } = await setup();

    const request = {
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 200_000n,
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
    expect(await mintAuthorizedDeposit(bridge, evm, first.Ok.deposit_id)).toHaveProperty("Ok.state.Minted");
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
  }
  it(
    "persists one idempotent Deposit through ledger pull, Mint Authorization, and finalized exact Mint evidence",
    persists_one_idempotent_Deposit_through_ledger_pull_Mint_Authorization_and_finalized_exact_Mint_evidence,
  );

  it("uses a stable owner sequence for deterministic replay, conflicts, and gaps", async () => {
    const { bridge, runtimePrincipal } = await setup();
    expect(await (bridge.actor as any).get_next_deposit_sequence(runtimePrincipal)).toBe(0n);
    const request = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n };
    const first: any = await (bridge.actor as any).request_deposit(request);
    expect(first).toHaveProperty("Ok");
    expect(first.Ok.owner_sequence).toBe(0n);
    expect(await (bridge.actor as any).get_next_deposit_sequence(runtimePrincipal)).toBe(1n);
    await advanceDepositJobs(bridge, first.Ok.deposit_id);
    const replay: any = await (bridge.actor as any).request_deposit(request);
    expect(Array.from(replay.Ok.deposit_id)).toEqual(Array.from(first.Ok.deposit_id));
    expect(await (bridge.actor as any).request_deposit({ ...request, gross_amount: 200_001n })).toEqual({ Err: { DepositConflict: null } });
    expect(await (bridge.actor as any).request_deposit({ ...request, owner_sequence: 2n })).toEqual({ Err: { SequenceMismatch: { expected: 1n } } });
  });

  it("rejects gross amounts at or below the fixed refund fee before record, sequence, or ledger use", async () => {
    const { ledger, bridge, runtimePrincipal } = await setup();
    const request = (gross_amount: bigint) => ({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount, max_service_fee: 10n });
    expect(await (bridge.actor as any).request_deposit(request(testLedgerFee))).toHaveProperty("Err.InvalidRequest");
    expect(await (bridge.actor as any).request_deposit(request(testLedgerFee - 1n))).toHaveProperty("Err.InvalidRequest");
    expect(await (bridge.actor as any).get_next_deposit_sequence(runtimePrincipal)).toBe(0n);
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(0);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
  });

  async function authenticated_relayer_refund_preserves_fixed_identity() {
    const { bridge, evm, ledger } = await setup();
    const owner = Principal.selfAuthenticating(new Uint8Array(32).fill(31));
    const thirdParty = Principal.selfAuthenticating(new Uint8Array(32).fill(32));

    bridge.actor.setPrincipal(owner);
    const deposit: any = await requestDefaultDeposit(bridge);
    const authorization = await awaitMintAuthorization(bridge, deposit.Ok.deposit_id);
    await evm.actor.set_block_timestamp(authorization.deadline);
    const boundary: any = await (bridge.actor as any).request_deposit_refund(deposit.Ok.deposit_id);
    expect(boundary).toEqual({ Err: { NotClaimable: null } });
    const callsAfterBoundary = await (evm.actor as any).eth_call_count();
    expect(await (bridge.actor as any).request_deposit_refund(deposit.Ok.deposit_id))
      .toEqual({ Err: { NotClaimable: null } });
    expect(await (evm.actor as any).eth_call_count()).toBe(callsAfterBoundary);
    let stored: any = await bridge.actor.get_deposit(deposit.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("AuthorizationAvailable");
    expect(stored[0].refund).toEqual([]);

    const callsBeforeRejectedIdentity = await (evm.actor as any).eth_call_count();
    const processedBeforeRejectedIdentity = await (evm.actor as any).deposit_processed_call_count();
    const ledgerBeforeRejectedIdentity = await (ledger.actor as any).ledger_transfer_calls();
    bridge.actor.setPrincipal(Principal.anonymous());
    expect(await (bridge.actor as any).request_deposit_refund(deposit.Ok.deposit_id)).toEqual({ Err: { AnonymousCaller: null } });
    expect(await (evm.actor as any).eth_call_count()).toBe(callsBeforeRejectedIdentity);
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(processedBeforeRejectedIdentity);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(ledgerBeforeRejectedIdentity);
    bridge.actor.setPrincipal(thirdParty);
    const callsBeforeExpiry = await (evm.actor as any).eth_call_count();
    const processedCallsBeforeExpiry = await (evm.actor as any).deposit_processed_call_count();
    await setExpiredBlockTimestamp(evm, authorization.deadline + 1n);
    expect(await (bridge.actor as any).request_deposit_refund(deposit.Ok.deposit_id)).toHaveProperty("Ok.state.Refunded");
    expect(await (evm.actor as any).eth_call_count()).toBeLessThanOrEqual(callsBeforeExpiry + 2n);
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(processedCallsBeforeExpiry + 1n);
    stored = await bridge.actor.get_deposit(deposit.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("Refunded");
    const transfer = (await (ledger.actor as any).ledger_transactions()).at(-1)?.transfer?.[0];
    expect(transfer.to.owner.toText()).toBe(owner.toText());
    expect(transfer.to.owner.toText()).not.toBe(thirdParty.toText());
    expect(transfer.amount).toBe(stored[0].refund[0].amount);
    const transfersAfterRefund = (await (ledger.actor as any).ledger_transactions()).length;
    expect(await (bridge.actor as any).request_deposit_refund(deposit.Ok.deposit_id))
      .toEqual({ Err: { NotClaimable: null } });
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(transfersAfterRefund);
  }

  it(
    "allows an authenticated relayer to start a fixed-identity refund after strict expiry",
    authenticated_relayer_refund_preserves_fixed_identity,
  );

  async function prepaid_refund_quota_is_not_charged_twice() {
    const { bridge, evm } = await setup(true, {
      settlement_rate_limit_global: 1,
      settlement_rate_limit_per_principal: 1,
      settlement_rate_limit_per_record: 1,
    });
    const owner = Principal.selfAuthenticating(new Uint8Array(32).fill(33));
    const relayer = Principal.selfAuthenticating(new Uint8Array(32).fill(34));
    bridge.actor.setPrincipal(owner);
    const deposit: any = await requestDefaultDeposit(bridge);
    const authorization = await awaitMintAuthorization(bridge, deposit.Ok.deposit_id);
    await evm.actor.set_processed_deposit(false);
    await setExpiredBlockTimestamp(evm, authorization.deadline + 1n);

    bridge.actor.setPrincipal(relayer);
    expect(await (bridge.actor as any).request_deposit_refund(deposit.Ok.deposit_id))
      .toHaveProperty("Ok.state.Refunded");
  }

  it(
    "prepays one settlement quota before refund RPC and does not charge the claim twice",
    prepaid_refund_quota_is_not_charged_twice,
  );

  async function failed_recovery_observation_keeps_quota_and_blocks_the_next_rpc() {
    const { bridge, evm } = await setup(true, {
      settlement_rate_limit_global: 1,
      settlement_rate_limit_per_principal: 1,
      settlement_rate_limit_per_record: 1,
    });
    const deposit: any = await requestDefaultDeposit(bridge);
    const authorization = await awaitMintAuthorization(bridge, deposit.Ok.deposit_id);
    await setExpiredBlockTimestamp(evm, authorization.deadline + 1n);
    await (evm.actor as any).set_block_mode({ FinalizedUnavailable: null });

    expect(await (bridge.actor as any).request_deposit_refund(deposit.Ok.deposit_id))
      .toEqual({ Err: { FinalityUnavailable: null } });
    const ethCallsAfterFailure = await (evm.actor as any).eth_call_count();
    const processedCallsAfterFailure = await (evm.actor as any).deposit_processed_call_count();
    const receiptCallsAfterFailure = await (evm.actor as any).receipt_call_count();
    expect(await (bridge.actor as any).request_deposit_refund(deposit.Ok.deposit_id))
      .toHaveProperty("Err.RateLimited");
    expect(await (evm.actor as any).eth_call_count()).toBe(ethCallsAfterFailure);
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(processedCallsAfterFailure);
    expect(await (evm.actor as any).receipt_call_count()).toBe(receiptCallsAfterFailure);
  }

  it(
    "keeps failed recovery observation quota and blocks another RPC at the limit",
    failed_recovery_observation_keeps_quota_and_blocks_the_next_rpc,
  );

  it("serializes concurrent owner refund claims and never sends two refunds", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const deposit: any = await requestDefaultDeposit(bridge);
    const authorization = await awaitMintAuthorization(bridge, deposit.Ok.deposit_id);
    await evm.actor.set_processed_deposit(false);
    await setExpiredBlockTimestamp(evm, authorization.deadline + 1n);

    const deferred = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    deferred.setPrincipal(runtimePrincipal);
    const first = await deferred.request_deposit_refund(deposit.Ok.deposit_id);
    const second = await deferred.request_deposit_refund(deposit.Ok.deposit_id);
    const results: any[] = await Promise.all([first(), second()]);

    expect(results.filter((result) => "Err" in result && "Busy" in result.Err)).toHaveLength(1);
    expect(results.filter((result) => "Ok" in result && "Refunded" in result.Ok.state)).toHaveLength(1);
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(2);
  });

  it("preserves externally relayed governance signatures across upgrade and only replaces explicitly", async () => {
    const { bridge, evm, init, runtimePrincipal } = await setup();
    const pauseAction = { PauseDepositMints: null };

    bridge.actor.setPrincipal(runtimePrincipal);
    const prepared: any = await (bridge.actor as any).prepare_base_governance_action(pauseAction);
    expect(prepared).toHaveProperty("Ok.raw_transaction");
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(0);

    await pic!.upgradeCanister({
      canisterId: bridge.canisterId,
      wasm: readFileSync(bridgeWasm),
      arg: IDL.encode([], []),
    });
    bridge.actor.setPrincipal(runtimePrincipal);
    const pending: any = await (bridge.actor as any).get_pending_base_governance_transaction();
    expect(Buffer.from(pending.Ok[0].raw_transaction)).toEqual(Buffer.from(prepared.Ok.raw_transaction));
    const bumped = (value: bigint) => (value * 11_250n + 9_999n) / 10_000n;
    const replacement: any = await (bridge.actor as any).prepare_base_governance_replacement({
      operation_id: prepared.Ok.operation_id,
      expected_transaction_hash: prepared.Ok.transaction_hash,
      max_fee_per_gas: bumped(prepared.Ok.max_fee_per_gas),
      max_priority_fee_per_gas: bumped(prepared.Ok.max_priority_fee_per_gas),
    });
    expect(replacement.Ok.generation).toBe(1);
    expect(Buffer.from(replacement.Ok.transaction_hash)).not.toEqual(Buffer.from(prepared.Ok.transaction_hash));
    expect(await (evm.actor as any).broadcast_transactions()).toHaveLength(0);
    let current = replacement.Ok;
    for (const generation of [2, 3]) {
      const next: any = await (bridge.actor as any).prepare_base_governance_replacement({
        operation_id: current.operation_id,
        expected_transaction_hash: current.transaction_hash,
        max_fee_per_gas: bumped(current.max_fee_per_gas),
        max_priority_fee_per_gas: bumped(current.max_priority_fee_per_gas),
      });
      expect(next.Ok.generation).toBe(generation);
      current = next.Ok;
    }
    expect(await (bridge.actor as any).prepare_base_governance_replacement({
      operation_id: current.operation_id,
      expected_transaction_hash: current.transaction_hash,
      max_fee_per_gas: current.max_fee_per_gas,
      max_priority_fee_per_gas: current.max_priority_fee_per_gas,
    })).toHaveProperty("Err.ReplacementLimitReached");

    expect(await (bridge.actor as any).confirm_base_governance_transaction({
      operation_id: prepared.Ok.operation_id,
      transaction_hash: new Uint8Array(32).fill(0xff),
    })).toHaveProperty("Err.InvalidArgument");
    await (evm.actor as any).set_receipt_mode({ Missing: null });
    expect(await (bridge.actor as any).confirm_base_governance_transaction({
      operation_id: prepared.Ok.operation_id,
      transaction_hash: prepared.Ok.transaction_hash,
    })).toHaveProperty("Err.TransactionNotFinalized");
    await pic!.advanceTime(31_000);
    await (evm.actor as any).set_receipt_mode({ Confirmed: null });
    const oldGenerationConfirmed: any = await (bridge.actor as any).confirm_base_governance_transaction({
      operation_id: prepared.Ok.operation_id,
      transaction_hash: prepared.Ok.transaction_hash,
    });
    expect(oldGenerationConfirmed).toHaveProperty("Ok.succeeded", true);
    expect(await (bridge.actor as any).prepare_base_governance_replacement({
      operation_id: prepared.Ok.operation_id,
      expected_transaction_hash: current.transaction_hash,
      max_fee_per_gas: bumped(current.max_fee_per_gas),
      max_priority_fee_per_gas: bumped(current.max_priority_fee_per_gas),
    })).toHaveProperty("Err.InvalidArgument");

    const latestPrepared: any = await (bridge.actor as any).prepare_base_governance_action({ PauseWithdrawals: null });
    const latestReplacement: any = await (bridge.actor as any).prepare_base_governance_replacement({
      operation_id: latestPrepared.Ok.operation_id,
      expected_transaction_hash: latestPrepared.Ok.transaction_hash,
      max_fee_per_gas: bumped(latestPrepared.Ok.max_fee_per_gas),
      max_priority_fee_per_gas: bumped(latestPrepared.Ok.max_priority_fee_per_gas),
    });
    const latestConfirmed: any = await (bridge.actor as any).confirm_base_governance_transaction({
      operation_id: latestReplacement.Ok.operation_id,
      transaction_hash: latestReplacement.Ok.transaction_hash,
    });
    expect(latestConfirmed).toHaveProperty("Ok.succeeded", true);

    const revertedPrepared: any = await (bridge.actor as any).prepare_base_governance_action({ PauseDepositMints: null });
    await (evm.actor as any).set_receipt_mode({ Reverted: null });
    expect(await (bridge.actor as any).confirm_base_governance_transaction({
      operation_id: revertedPrepared.Ok.operation_id,
      transaction_hash: revertedPrepared.Ok.transaction_hash,
    })).toHaveProperty("Err.TransactionReverted");
    expect((await (bridge.actor as any).get_pending_base_governance_transaction()).Ok).toEqual([]);
    await (evm.actor as any).set_receipt_mode({ Confirmed: null });

    bridge.actor.setPrincipal(init.pause_principal);
    expect((await (bridge.actor as any).get_pending_base_governance_transaction()).Ok).toEqual([]);
  });

  async function governance_affordability_preserves_nonce_and_replacement_generation() {
    const { bridge, evm, runtimePrincipal } = await setup(false);
    bridge.actor.setPrincipal(runtimePrincipal);
    await (evm.actor as any).set_eth_balance(0n);

    const rejected: any = await (bridge.actor as any).prepare_base_governance_action({ PauseDepositMints: null });
    expect(rejected).toHaveProperty("Err.InsufficientGovernanceBalance.observed_wei", 0n);
    expect(rejected.Err.InsufficientGovernanceBalance.required_wei).toBeGreaterThan(0n);
    expect((await (bridge.actor as any).get_pending_base_governance_transaction()).Ok).toEqual([]);

    await (evm.actor as any).set_eth_balance(10_000_000_000_000_000_000n);
    const prepared: any = await (bridge.actor as any).prepare_base_governance_action({ PauseDepositMints: null });
    expect(prepared).toHaveProperty("Ok.nonce", 0n);
    expect(prepared).toHaveProperty("Ok.operation_id", 0n);

    const pendingBefore = await (bridge.actor as any).get_pending_base_governance_transaction();
    const bumped = (value: bigint) => (value * 11_250n + 9_999n) / 10_000n;
    await (evm.actor as any).set_eth_balance(0n);
    const replacement: any = await (bridge.actor as any).prepare_base_governance_replacement({
      operation_id: prepared.Ok.operation_id,
      expected_transaction_hash: prepared.Ok.transaction_hash,
      max_fee_per_gas: bumped(prepared.Ok.max_fee_per_gas),
      max_priority_fee_per_gas: bumped(prepared.Ok.max_priority_fee_per_gas),
    });
    expect(replacement).toHaveProperty("Err.InsufficientGovernanceBalance.observed_wei", 0n);
    expect(await (bridge.actor as any).get_pending_base_governance_transaction()).toEqual(pendingBefore);
  }

  it(
    "rejects unaffordable governance signatures without consuming the nonce or replacement generation",
    governance_affordability_preserves_nonce_and_replacement_generation,
  );

  async function governance_nonce_uses_configured_chain_without_runtime_probe() {
    const { bridge, evm, runtimePrincipal } = await setup();
    bridge.actor.setPrincipal(runtimePrincipal);

    expect(await (bridge.actor as any).prepare_base_governance_action({ PauseDepositMints: null }))
      .toHaveProperty("Ok.chain_id", 8453n);
    expect(await (evm.actor as any).chain_id_call_count()).toBe(0n);
  }

  it(
    "binds the governance nonce to configured chain without a runtime chain probe",
    governance_nonce_uses_configured_chain_without_runtime_probe,
  );

  async function keeps_signing_privileged_while_restricting_confirmation_callers() {
    const { bridge, evm, init, runtimePrincipal, confirmationRelayerPrincipal } = await setup(false);
    const pausePrincipal = init.pause_principal;
    const thirdParty = Principal.selfAuthenticating(new Uint8Array(32).fill(99));
    await (evm.actor as any).set_deposit_mints_paused(true);
    await (evm.actor as any).set_withdrawals_paused(true);
    await (evm.actor as any).set_receipt_mode({ Confirmed: null });

    bridge.actor.setPrincipal(runtimePrincipal);
    const schedule: any = await (bridge.actor as any).schedule_activation();
    expect(schedule).toHaveProperty("Ok.kind.ScheduleActivation");
    const bumped = (value: bigint) => (value * 11_250n + 9_999n) / 10_000n;
    bridge.actor.setPrincipal(Principal.anonymous());
    expect(await (bridge.actor as any).get_pending_base_governance_transaction())
      .toHaveProperty("Ok.0.kind.ScheduleActivation");
    expect(await (bridge.actor as any).prepare_base_governance_replacement({
      operation_id: schedule.Ok.operation_id,
      expected_transaction_hash: schedule.Ok.transaction_hash,
      max_fee_per_gas: bumped(schedule.Ok.max_fee_per_gas),
      max_priority_fee_per_gas: bumped(schedule.Ok.max_priority_fee_per_gas),
    })).toEqual({ Err: { Unauthorized: null } });
    const receiptCallsBeforeUnauthorized = await (evm.actor as any).receipt_call_count();
    expect(await (bridge.actor as any).confirm_base_governance_transaction({
      operation_id: schedule.Ok.operation_id,
      transaction_hash: schedule.Ok.transaction_hash,
    })).toEqual({ Err: { Unauthorized: null } });
    expect(await (evm.actor as any).receipt_call_count()).toBe(receiptCallsBeforeUnauthorized);
    bridge.actor.setPrincipal(thirdParty);
    expect(await (bridge.actor as any).confirm_base_governance_transaction({
      operation_id: schedule.Ok.operation_id,
      transaction_hash: schedule.Ok.transaction_hash,
    })).toEqual({ Err: { Unauthorized: null } });
    expect(await (evm.actor as any).receipt_call_count()).toBe(receiptCallsBeforeUnauthorized);
    bridge.actor.setPrincipal(confirmationRelayerPrincipal);
    expect(await (bridge.actor as any).confirm_base_governance_transaction({
      operation_id: schedule.Ok.operation_id,
      transaction_hash: schedule.Ok.transaction_hash,
    })).toHaveProperty("Ok.succeeded", true);
    expect(await (bridge.actor as any).prepare_base_governance_action({ PauseDepositMints: null }))
      .toEqual({ Err: { Unauthorized: null } });
    expect(await (bridge.actor as any).prepare_next_emergency_base_action())
      .toEqual({ Err: { Unauthorized: null } });
    bridge.actor.setPrincipal(Principal.anonymous());
    expect(await (bridge.actor as any).prepare_next_emergency_base_action())
      .toEqual({ Err: { Unauthorized: null } });

    bridge.actor.setPrincipal(pausePrincipal);
    expect(await (bridge.actor as any).prepare_base_governance_action({
      SetServiceFee: { value: 1n },
    })).toEqual({ Err: { Unauthorized: null } });
    expect(await (bridge.actor as any).schedule_activation()).toEqual({ Err: { Unauthorized: null } });
    expect(await (bridge.actor as any).execute_activation()).toEqual({ Err: { Unauthorized: null } });
    expect(await (bridge.actor as any).emergency_pause()).toHaveProperty("Ok.base_actions_queued", true);

    const expectedKinds = ["PauseDepositMints", "PauseWithdrawals", "CancelTimelock"];
    for (const [index, expectedKind] of expectedKinds.entries()) {
      const prepared: any = await (bridge.actor as any).prepare_next_emergency_base_action();
      expect(prepared).toHaveProperty(`Ok.kind.${expectedKind}`);
      expect(await (bridge.actor as any).get_pending_base_governance_transaction())
        .toHaveProperty(`Ok.0.kind.${expectedKind}`);
      let transaction = prepared.Ok;
      if (index === 0) {
        const replacement: any = await (bridge.actor as any).prepare_base_governance_replacement({
          operation_id: transaction.operation_id,
          expected_transaction_hash: transaction.transaction_hash,
          max_fee_per_gas: bumped(transaction.max_fee_per_gas),
          max_priority_fee_per_gas: bumped(transaction.max_priority_fee_per_gas),
        });
        expect(replacement).toHaveProperty("Ok.generation", 1);
        transaction = replacement.Ok;
      }
      expect(await (bridge.actor as any).confirm_base_governance_transaction({
        operation_id: transaction.operation_id,
        transaction_hash: transaction.transaction_hash,
      })).toHaveProperty("Ok.succeeded", true);
    }
    expect(await (bridge.actor as any).prepare_next_emergency_base_action())
      .toEqual({ Err: { InvalidArgument: null } });

    bridge.actor.setPrincipal(thirdParty);
    expect(await (bridge.actor as any).prepare_base_governance_action({ PauseDepositMints: null }))
      .toEqual({ Err: { Unauthorized: null } });
    expect(await (bridge.actor as any).get_pending_base_governance_transaction())
      .toEqual({ Ok: [] });
    expect(await (bridge.actor as any).prepare_next_emergency_base_action())
      .toEqual({ Err: { Unauthorized: null } });
  }

  it(
    "keeps signing privileged while restricting confirmation callers",
    keeps_signing_privileged_while_restricting_confirmation_callers,
  );

  async function reauthorizes_confirmation_after_external_receipt_observation() {
    const { bridge, evm, init, runtimePrincipal, confirmationRelayerPrincipal } = await setup(false);
    const nextPausePrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(65));
    bridge.actor.setPrincipal(runtimePrincipal);
    const scheduled: any = await (bridge.actor as any).schedule_activation();
    expect(scheduled).toHaveProperty("Ok.kind.ScheduleActivation");
    await (evm.actor as any).set_receipt_mode({ DelayedConfirmed: null });
    const receiptCallsBefore = await (evm.actor as any).receipt_call_count();

    const deferredConfirmation = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    deferredConfirmation.setPrincipal(init.pause_principal);
    const startConfirmation = await deferredConfirmation.confirm_base_governance_transaction({
      operation_id: scheduled.Ok.operation_id,
      transaction_hash: scheduled.Ok.transaction_hash,
    });
    let receiptBarrier: Awaited<ReturnType<NonNullable<typeof pic>["getPendingHttpsOutcalls"]>>[number] | undefined;
    for (let attempt = 0; attempt < 10; attempt += 1) {
      await pic!.tick(1);
      receiptBarrier = (await pic!.getPendingHttpsOutcalls())
        .find((outcall) => outcall.url === "https://receipt-delay.invalid/");
      if (receiptBarrier) break;
    }
    expect(receiptBarrier).toBeDefined();
    expect(await (evm.actor as any).receipt_call_count()).toBeGreaterThan(receiptCallsBefore);

    bridge.actor.setPrincipal(runtimePrincipal);
    const rotation = await (bridge.actor as any).rotate_pause_principal({
      pause_principal: nextPausePrincipal,
    });
    expect(rotation).toHaveProperty("Ok");
    expect((await (bridge.actor as any).get_operational_config()).Ok.pause_principal.toText())
      .toBe(nextPausePrincipal.toText());
    await pic!.mockPendingHttpsOutcall({
      requestId: receiptBarrier!.requestId,
      subnetId: receiptBarrier!.subnetId,
      response: { type: "success", statusCode: 200, headers: [], body: new Uint8Array() },
    });
    const revokedConfirmation = await startConfirmation();
    expect(revokedConfirmation).toEqual({ Err: { Unauthorized: null } });
    expect(await (bridge.actor as any).get_pending_base_governance_transaction())
      .toHaveProperty("Ok.0.kind.ScheduleActivation");

    await (evm.actor as any).set_receipt_mode({ Confirmed: null });
    bridge.actor.setPrincipal(confirmationRelayerPrincipal);
    expect(await (bridge.actor as any).confirm_base_governance_transaction({
      operation_id: scheduled.Ok.operation_id,
      transaction_hash: scheduled.Ok.transaction_hash,
    })).toHaveProperty("Ok.succeeded", true);
  }

  it(
    "reauthorizes confirmation after external receipt observation",
    reauthorizes_confirmation_after_external_receipt_observation,
  );

  it("binds a selected subaccount, exposes only runtime binding, protects operational configuration, consent, and owner history", async () => {
    const { bridge, init, runtimePrincipal } = await setup();
    const selectedSubaccount = new Uint8Array(32).fill(8);
    const request = {
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(9),
      from_subaccount: [selectedSubaccount],
      gross_amount: 200_000n,
      max_service_fee: 10n,
    };
    const depositActor = pic!.createActor(bridgeIdl, bridge.canisterId);
    depositActor.setPrincipal(runtimePrincipal);

    const standards: any = await (bridge.actor as any).icrc10_supported_standards();
    expect(standards).toEqual([{ name: "ICRC-21", url: "https://github.com/dfinity/ICRC/blob/main/ICRCs/ICRC-21/ICRC-21.md" }]);
    const config: any = await (bridge.actor as any).get_runtime_binding();
    expect(config.base_chain_id).toBe(8453n);
    expect(config.schema_version).toBe(35);
    expect(Array.from(config.minimum_withdrawal_id)).toEqual(Array.from(init.minimum_withdrawal_id));
    expect(config.ledger_canister_id.toText()).toBe(init.ledger_canister_id.toText());
    expect(config.evm_rpc_canister_id.toText()).toBe(init.evm_rpc_canister_id.toText());
    expect(config.rpc_provider_urls_sha256).toHaveLength(32);
    expect(config.operational_config_sha256).toHaveLength(32);
    expect(config).not.toHaveProperty("confirmation_relayer_principal");
    bridge.actor.setPrincipal(Principal.anonymous());
    expect(await (bridge.actor as any).get_operational_config()).toEqual({ Err: { Unauthorized: null } });
    bridge.actor.setPrincipal(runtimePrincipal);
    const operational: any = await (bridge.actor as any).get_operational_config();
    expect(operational).toHaveProperty("Ok");
    expect(operational.Ok.notification_rate_limit_window_seconds).toBe(600n);
    expect(operational.Ok.notification_rate_limit_global).toBe(60);
    expect(operational.Ok.notification_ingestion_rate_limit_global).toBe(30);

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

  async function publishes_rotated_administrator_configuration_and_preserves_it_across_reopen() {
    const { bridge, init, runtimePrincipal } = await setup(false);
    const nextPausePrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(65));
    const nextFeeRecipient = Principal.selfAuthenticating(new Uint8Array(32).fill(66));
    const initialBinding: any = await (bridge.actor as any).get_runtime_binding();

    expect(await (bridge.actor as any).rotate_pause_principal({
      pause_principal: nextPausePrincipal,
    })).toHaveProperty("Ok");
    expect(await (bridge.actor as any).rotate_fee_recipient({
      owner: nextFeeRecipient,
      subaccount: [],
    })).toHaveProperty("Ok");

    const rotated: any = await (bridge.actor as any).get_operational_config();
    const rotatedBinding: any = await (bridge.actor as any).get_runtime_binding();
    expect(rotated.Ok.pause_principal.toText()).toBe(nextPausePrincipal.toText());
    expect(rotated.Ok.fee_recipient.owner.toText()).toBe(nextFeeRecipient.toText());
    expect(rotatedBinding.operational_config_sha256).not.toEqual(initialBinding.operational_config_sha256);

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
    const reopened: any = await (bridge.actor as any).get_operational_config();
    const reopenedBinding: any = await (bridge.actor as any).get_runtime_binding();
    expect(reopened.Ok.pause_principal.toText()).toBe(nextPausePrincipal.toText());
    expect(reopened.Ok.fee_recipient.owner.toText()).toBe(nextFeeRecipient.toText());
    expect(reopenedBinding.operational_config_sha256).toEqual(rotatedBinding.operational_config_sha256);
  }

  it(
    "publishes rotated administrator configuration and preserves it across reopen",
    publishes_rotated_administrator_configuration_and_preserves_it_across_reopen,
  );

  it("refuses to sign when the Base service fee changes after funded preflight", async () => {
    const { ledger, evm, bridge } = await setup();
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    expect(result).toHaveProperty("Ok");
    await (evm.actor as any).set_service_fee(7n);
    await advanceDepositJobs(bridge, result.Ok.deposit_id);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(stored[0].quote[0].service_fee).toBe(1n);
    expect(stored[0].quote[0].net_amount).toBe(199_999n);
    expect(phaseName(stored[0].state)).toBe("AuthorizationPending");
    expect(stored[0].mint_authorization[0].signature).toEqual([]);
    expect(stored[0].last_settlement_stop_reason[0]).toEqual({ InvalidBaseResponse: null });
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(1);
  });

  it("promotes funded deposits before refunding a later Mint window overflow", async () => {
    const { ledger, evm, bridge } = await setup();
    const baseNow = BigInt(Math.floor((await pic!.getTime()) / 1_000));
    await (evm.actor as any).set_mint_window(0n, 300_000n, baseNow, 100n, baseNow + 1n);
    const first: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    expect(first).toHaveProperty("Ok");
    await awaitMintAuthorization(bridge, first.Ok.deposit_id);
    await (evm.actor as any).set_mint_window(
      199_993n,
      300_000n,
      baseNow,
      100n,
      baseNow + 1n,
    );
    const second: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_003n, max_service_fee: 10n });
    expect(second).toHaveProperty("Ok.state.EscrowedUnquoted");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(2);
    await advanceDepositJobs(bridge, second.Ok.deposit_id);
    const stored: any = await (bridge.actor as any).get_deposit(second.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("RefundAvailable");
    expect(stored[0].available_refund_amount).toEqual([190_003n]);
  });

  async function processed_candidate_is_checked_after_funding_and_preserves_the_funded_record() {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_processed_deposit(true);
    const ledgerCallsBefore = await (ledger.actor as any).ledger_transfer_calls();
    const baseCallsBefore = await (evm.actor as any).deposit_processed_call_count();

    const funded: any = await requestDefaultDeposit(bridge);
    expect(funded).toHaveProperty("Ok.state.EscrowedUnquoted");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(ledgerCallsBefore + 1n);
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(baseCallsBefore + 1n);
    expect(await (ledger.actor as any).ledger_transactions()).toHaveLength(1);

    const replay: any = await requestDefaultDeposit(bridge);
    expect(replay).toHaveProperty("Ok.deposit_id");
    expect(replay.Ok.deposit_id).toEqual(funded.Ok.deposit_id);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(ledgerCallsBefore + 1n);
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(baseCallsBefore + 1n);
  }

  it(
    "checks a processed candidate only after funding and preserves the funded record",
    processed_candidate_is_checked_after_funding_and_preserves_the_funded_record,
  );

  it("does not make Deposit admission or Mint Authorization depend on the Mint Signer ETH balance", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_eth_balance(0n);
    const accepted: any = await (bridge.actor as any).request_deposit({
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 200_000n,
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
    const accepted: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    await awaitMintAuthorization(bridge, accepted.Ok.deposit_id);
    await (ledger.actor as any).set_refund_ledger_mode([{ Trap: null }]);
    expect((await expireUnusedAuthorization(bridge, evm, accepted.Ok.deposit_id)).result)
      .toHaveProperty("Ok.state.RefundProcessing");
    const held: any = await (bridge.actor as any).get_deposit(accepted.Ok.deposit_id);
    expect(phaseName(held[0].state)).toBe("RefundProcessing");
    expect(held[0].refund[0]).toMatchObject({ amount: 189_999n, ledger_fee: testLedgerFee, attempt_no: 0n });
    expect(held[0].refund[0].status).toEqual({ ReconciliationRequired: null });
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(1);

    await (ledger.actor as any).set_refund_ledger_mode([{ Succeed: null }]);
    await advancePastReconciliationDelay();
    expect(await (bridge.actor as any).request_deposit_refund(accepted.Ok.deposit_id)).toHaveProperty("Ok.state.Refunded");
    const refunded: any = await (bridge.actor as any).get_deposit(accepted.Ok.deposit_id);
    expect(phaseName(refunded[0].state)).toBe("Refunded");
    expect(refunded[0].refund[0]).toMatchObject({ amount: 189_999n, ledger_fee: testLedgerFee, attempt_no: 0n });
    expect(refunded[0].refund[0].status).toEqual({ Completed: null });
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(2);
  });

  it("creates a new fixed-payload refund attempt only after complete absence proof", async () => {
    const { ledger, index, evm, bridge } = await setup();
    const accepted: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    await awaitMintAuthorization(bridge, accepted.Ok.deposit_id);
    await (ledger.actor as any).set_refund_ledger_mode([{ Trap: null }]);
    expect((await expireUnusedAuthorization(bridge, evm, accepted.Ok.deposit_id)).result)
      .toHaveProperty("Ok.state.RefundProcessing");
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(1);

    await (index.actor as any).set_index_synced_blocks([1n]);
    await pic!.advanceTime(24 * 60 * 60 * 1_000 + 60_001);
    await (ledger.actor as any).set_refund_ledger_mode([{ Succeed: null }]);
    expect(await (bridge.actor as any).request_deposit_refund(accepted.Ok.deposit_id)).toHaveProperty("Ok.state.Refunded");
    const refunded: any = await (bridge.actor as any).get_deposit(accepted.Ok.deposit_id);
    expect(phaseName(refunded[0].state)).toBe("Refunded");
    expect(refunded[0].refund[0]).toMatchObject({ amount: 189_999n, ledger_fee: testLedgerFee, attempt_no: 1n });
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(2);
  });

  it("stops a refund BadFee without changing the fixed refund payload", async () => {
    const { ledger, evm, bridge } = await setup();
    const accepted: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    await awaitMintAuthorization(bridge, accepted.Ok.deposit_id);
    await (ledger.actor as any).set_ledger_fee(12_000n);
    await (ledger.actor as any).set_refund_ledger_mode([{ BadFee: null }]);
    expect((await expireUnusedAuthorization(bridge, evm, accepted.Ok.deposit_id)).result)
      .toHaveProperty("Ok.state.RefundProcessing");
    const stopped: any = await (bridge.actor as any).get_deposit(accepted.Ok.deposit_id);
    expect(phaseName(stopped[0].state)).toBe("RefundProcessing");
    expect(stopped[0].refund[0]).toMatchObject({ amount: 189_999n, ledger_fee: testLedgerFee, attempt_no: 0n });
    expect(stopped[0].refund[0].status).toEqual({ Sending: null });
    expect(stopped[0].last_settlement_stop_reason[0].LedgerRejected).toContain("BadFee");

    await (ledger.actor as any).set_refund_ledger_mode([{ Succeed: null }]);
    expect(await (bridge.actor as any).request_deposit_refund(accepted.Ok.deposit_id)).toHaveProperty("Ok.state.Refunded");
    const refunded: any = await (bridge.actor as any).get_deposit(accepted.Ok.deposit_id);
    expect(refunded[0].refund[0]).toMatchObject({ amount: 189_999n, ledger_fee: testLedgerFee, attempt_no: 0n });
  });

  it("treats a full expired Mint window as having zero effective consumption", async () => {
    const { ledger, evm, bridge } = await setup();
    const baseNow = BigInt(Math.floor((await pic!.getTime()) / 1_000));
    await (evm.actor as any).set_mint_window(
      300_000n,
      300_000n,
      baseNow,
      10n,
      baseNow + 11n,
    );
    const accepted: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    expect(accepted).toHaveProperty("Ok");
    await awaitMintAuthorization(bridge, accepted.Ok.deposit_id);
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
  });

  it("funds before stale Mint observation and withholds authorization until recovery", async () => {
    const { ledger, evm, bridge } = await setup();
    const seed: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    await mintAuthorizedDeposit(bridge, evm, seed.Ok.deposit_id);
    expect(phaseName((await (bridge.actor as any).get_deposit(seed.Ok.deposit_id))[0].state)).toBe("Minted");

    await pic!.advanceTime(61_000);
    await (evm.actor as any).set_finalized_block_sequence(Array(64).fill(98n));
    const stale: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    expect(stale).toHaveProperty("Ok.state.EscrowedUnquoted");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(2);
    await advanceDepositJobs(bridge, stale.Ok.deposit_id);
    const withheld: any = await (bridge.actor as any).get_deposit(stale.Ok.deposit_id);
    expect(phaseName(withheld[0].state)).toBe("EscrowedUnquoted");
    expect(withheld[0].mint_authorization).toEqual([]);

    await (evm.actor as any).set_finalized_block(101n, new Uint8Array(32).fill(0x12));
    await (evm.actor as any).set_block_timestamp(BigInt(Math.floor((await pic!.getTime()) / 1_000)));
    await advanceClock(2);
    await awaitMintAuthorization(bridge, stale.Ok.deposit_id);
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(2);
  });

  it("uses one fresh Finalized observation for every Deposit candidate", async () => {
    const { evm, bridge } = await setup();
    const before = await (evm.actor as any).eth_call_count();
    const processedBefore = await (evm.actor as any).deposit_processed_call_count();
    const first: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    const afterFirst = await (evm.actor as any).eth_call_count();
    expect(afterFirst).toBeGreaterThan(before);
    await advanceDepositJobs(bridge, first.Ok.deposit_id);
    const afterQuote = await (evm.actor as any).eth_call_count();
    const second: any = await (bridge.actor as any).request_deposit({ owner_sequence: 1n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    const afterSecond = await (evm.actor as any).eth_call_count();
    expect(afterQuote).toBeGreaterThan(afterFirst);
    expect(afterSecond).toBeGreaterThan(afterQuote);
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(processedBefore + 2n);
    expect(second).toHaveProperty("Ok.state.EscrowedUnquoted");
  });

  it("keeps the committed Deposit observation when Base pauses after preflight", async () => {
    const { ledger, evm, bridge } = await setup();
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    expect(result).toHaveProperty("Ok.state.EscrowedUnquoted");
    await (evm.actor as any).set_deposit_mints_paused(true);
    await advanceDepositJobs(bridge, result.Ok.deposit_id);
    const record: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(phaseName(record[0].state)).toBe("AuthorizationPending");
    expect(record[0].available_refund_amount).toEqual([]);
    expect(record[0].refund).toEqual([]);
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("keeps the committed Deposit signer observation when Base rotates after preflight", async () => {
    const { ledger, evm, bridge } = await setup();
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    expect(result).toHaveProperty("Ok.state.EscrowedUnquoted");
    expect(await (evm.actor as any).set_bridge_signer(new Uint8Array(20).fill(0xaa))).toHaveProperty("Ok");
    await advanceDepositJobs(bridge, result.Ok.deposit_id);
    const record: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(phaseName(record[0].state)).toBe("AuthorizationPending");
    expect(record[0].refund).toEqual([]);
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  async function stops_a_less_than_300_second_pending_authorization_before_installing_a_signature() {
    const { evm, bridge } = await setup();
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    expect(result).toHaveProperty("Ok.state.EscrowedUnquoted");
    await (evm.actor as any).set_block_mode({ FinalizedUnavailable: null });
    await advanceDepositJobs(bridge, result.Ok.deposit_id);

    const pending: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(phaseName(pending[0].state)).toBe("AuthorizationPending");
    expect(pending[0].mint_authorization[0].signature).toEqual([]);
    const deadline = BigInt(pending[0].mint_authorization[0].deadline);
    const now = BigInt(Math.floor((await pic!.getTime()) / 1_000));
    expect(deadline - now).toBeLessThan(300n);
    await (evm.actor as any).set_block_mode({ Canonical: null });
    await pic!.tick(30);

    const stopped: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(phaseName(stopped[0].state)).toBe("AuthorizationPending");
    expect(stopped[0].mint_authorization[0].signature).toEqual([]);
    expect(stopped[0].last_settlement_stop_reason).toEqual([{ AuthorizationWindowTooShort: null }]);
  }

  it(
    "stops_a_less_than_300_second_pending_authorization_before_installing_a_signature",
    stops_a_less_than_300_second_pending_authorization_before_installing_a_signature,
  );

  async function continues_only_a_retryable_stopped_deposit_authorization() {
    const { evm, bridge, runtimePrincipal } = await setup();
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    expect(result).toHaveProperty("Ok.state.EscrowedUnquoted");

    await (evm.actor as any).set_block_mode({ FinalizedUnavailable: null });
    // The setup observation is still fresh enough to issue the authorization.
    // Stop on the immediately following Base revalidation attempt, before the
    // 300-second signing floor becomes the dominant stop reason.
    await advanceClock(2);
    await (evm.actor as any).set_block_mode({ Canonical: null });
    const stopped: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(stopped[0].last_settlement_stop_reason).toEqual([{ RpcUnavailable: null }]);

    bridge.actor.setPrincipal(Principal.anonymous());
    expect(await (bridge.actor as any).continue_deposit(result.Ok.deposit_id))
      .toEqual({ Err: { AnonymousCaller: null } });
    bridge.actor.setPrincipal(runtimePrincipal);
    const automatic: any = await (bridge.actor as any).continue_deposit(result.Ok.deposit_id);
    expect(automatic).toHaveProperty("Err.AutomaticProgressPending");
    const nextRunAtNs = automatic.Err.AutomaticProgressPending.next_run_at_ns[0];
    expect(nextRunAtNs).toBeDefined();
    const nowNs = BigInt(await pic!.getTime()) * 1_000_000n;
    const manualRetryAtNs = nextRunAtNs + 300_000_000_000n + 1_000_000n;
    await (evm.actor as any).set_block_timestamp(manualRetryAtNs / 1_000_000_000n);
    await pic!.advanceTime(Number((manualRetryAtNs - nowNs) / 1_000_000n));
    expect(await (bridge.actor as any).continue_deposit(result.Ok.deposit_id))
      .toHaveProperty("Ok.Stopped.reason.AuthorizationWindowTooShort");

    const resumed: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(phaseName(resumed[0].state)).toBe("AuthorizationPending");
    expect(resumed[0].last_settlement_stop_reason).toEqual([{ AuthorizationWindowTooShort: null }]);
    expect(await (bridge.actor as any).continue_deposit(result.Ok.deposit_id))
      .toEqual({ Err: { WrongState: null } });
  }

  it(
    "continues only a retryable stopped deposit authorization",
    continues_only_a_retryable_stopped_deposit_authorization,
  );

  it("rate-limits new deposit admissions while preserving idempotent retries", async () => {
    const { bridge } = await setup();
    const request = (tag: number) => ({ owner_sequence: BigInt(tag - 72), base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
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

  async function definitive_funding_failures_never_reach_Base_RPC_or_create_formal_deposits() {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    await (ledger.actor as any).set_ledger_mode({ InsufficientAllowance: { allowance: 0n } });
    const callsBefore = await (evm.actor as any).deposit_processed_call_count();
    const request = {
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 200_000n,
      max_service_fee: 10n,
    };
    for (let index = 0; index < 3; index += 1) {
      const rejected: any = await (bridge.actor as any).request_deposit({
        ...request,
      });
      expect(rejected).toHaveProperty("Err.FundingRejected.InsufficientAllowance.allowance", 0n);
    }
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(callsBefore);
    expect(await (bridge.actor as any).request_deposit(request)).toHaveProperty(
      "Err.FundingRejected.InsufficientAllowance.allowance",
      0n,
    );
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(callsBefore);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(4n);
    expect(await (bridge.actor as any).get_next_deposit_sequence(runtimePrincipal)).toBe(0n);
    expect((await (bridge.actor as any).list_deposit_ids({
      owner: runtimePrincipal,
      before_cursor: [],
      limit: 20,
    })).Ok.deposit_ids).toEqual([]);

    await advanceClock(2);
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    const funded: any = await (bridge.actor as any).request_deposit(request);
    expect(funded).toHaveProperty("Ok.state.EscrowedUnquoted");
    expect((await (bridge.actor as any).get_bridge_status()).counts.deposits).toBe(1n);
  }

  it(
    "keeps definitive funding failures outside Base RPC and formal Deposit state",
    definitive_funding_failures_never_reach_Base_RPC_or_create_formal_deposits,
  );

  async function unfunded_principals_never_reach_Base_RPC_across_global_admission_volume() {
    const { ledger, evm, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode({ InsufficientFunds: { balance: 0n } });
    const callsBefore = await (evm.actor as any).deposit_processed_call_count();
    for (let index = 0; index < 30; index += 1) {
      bridge.actor.setPrincipal(
        Principal.selfAuthenticating(new Uint8Array(32).fill(Math.floor(index / 3) + 80)),
      );
      expect(await (bridge.actor as any).request_deposit({
        owner_sequence: 0n,
        base_recipient: new Uint8Array(20).fill(4),
        from_subaccount: [],
        gross_amount: 200_000n,
        max_service_fee: 10n,
      })).toHaveProperty("Err.FundingRejected.InsufficientFunds.balance", 0n);
    }
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(callsBefore);

    bridge.actor.setPrincipal(Principal.selfAuthenticating(new Uint8Array(32).fill(120)));
    expect(await (bridge.actor as any).request_deposit({
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 200_000n,
      max_service_fee: 10n,
    })).toHaveProperty("Err.FundingRejected.InsufficientFunds.balance", 0n);
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(callsBefore);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(31n);
  }

  it(
    "keeps unfunded principals outside Base RPC across global admission volume",
    unfunded_principals_never_reach_Base_RPC_across_global_admission_volume,
  );

  async function rejects_insufficient_cycle_reserve_before_Deposit_Base_RPC_or_Ledger_pull() {
    const { ledger, evm, bridge } = await setup(true, {
      cycles_floor: 1n,
      settlement_cycle_ceiling: 200_000_000_000_000n,
    });
    await (ledger.actor as any).set_ledger_mode({ TemporarilyUnavailable: null });
    const request = {
      owner_sequence: 0n,
      base_recipient: new Uint8Array(20).fill(4),
      from_subaccount: [],
      gross_amount: 200_000n,
      max_service_fee: 10n,
    };
    for (const principalTag of [121, 122]) {
      const owner = Principal.selfAuthenticating(new Uint8Array(32).fill(principalTag));
      bridge.actor.setPrincipal(owner);
      expect(await (bridge.actor as any).request_deposit(request)).toHaveProperty("Err.FundingUnavailable");
      const open: any = await (bridge.actor as any).list_nonterminal_deposit_refs({
        owner,
        before_cursor: [],
        limit: 100,
      });
      expect(open.Ok.deposits).toHaveLength(1);
      expect(open.Ok.deposits[0].owner_sequence).toBe(0n);
      expect(await (bridge.actor as any).get_deposit(open.Ok.deposits[0].deposit_id)).toEqual([]);
    }
    bridge.actor.setPrincipal(Principal.selfAuthenticating(new Uint8Array(32).fill(123)));
    const baseCallsBefore = await (evm.actor as any).deposit_processed_call_count();
    const ledgerCallsBefore = await (ledger.actor as any).ledger_transfer_calls();
    expect(await (bridge.actor as any).request_deposit(request)).toEqual({ Err: { ReserveUnavailable: null } });
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(baseCallsBefore);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(ledgerCallsBefore);
  }

  it(
    "rejects insufficient cycle reserve before Deposit Base RPC or Ledger pull",
    rejects_insufficient_cycle_reserve_before_Deposit_Base_RPC_or_Ledger_pull,
  );

  it("rejects locally paused admissions before pull while preserving accepted replay", async () => {
    const { ledger, evm, bridge, init, runtimePrincipal } = await setup();
    const args = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n };
    bridge.actor.setPrincipal(init.pause_principal);
    await (bridge.actor as any).pause_new_deposits();
    bridge.actor.setPrincipal(runtimePrincipal);
    expect(await (bridge.actor as any).request_deposit(args)).toEqual({ Err: { DepositsPaused: null } });
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(0);

    await activateBridgeThroughGovernance(bridge, evm, runtimePrincipal);
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
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 1_000_000n, max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n }]);
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
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 1_000_000n, max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n }]);
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

  async function rejects_a_pre_boundary_withdrawal_before_state_or_Ledger_effects_and_accepts_the_boundary() {
    const minimum = new Uint8Array([...new Uint8Array(31), 6]);
    const { ledger, evm, bridge, runtimePrincipal } = await setup(true, {
      minimum_withdrawal_id: minimum,
    });
    const withdrawal = (id: Uint8Array) => [{
      id,
      owner: runtimePrincipal.toUint8Array(),
      subaccount: new Uint8Array(32),
      amount: 1_000_000n,
      max_service_fee: 100_000n,
      charged_service_fee: 100_000n,
      amount_out: 900_000n,
    }];
    const oldId = new Uint8Array([...new Uint8Array(31), 5]);
    await (evm.actor as any).set_withdrawal(withdrawal(oldId));

    expect(await (bridge.actor as any).notify_withdrawal({
      transaction_hash: new Uint8Array(32).fill(9),
    })).toEqual({
      Err: {
        WithdrawalBeforeAdmissionBoundary: {
          observed_withdrawal_id: oldId,
          minimum_withdrawal_id: minimum,
        },
      },
    });
    expect((await (bridge.actor as any).get_bridge_status()).counts.withdrawals).toBe(0n);
    expect(await (bridge.actor as any).get_withdrawal(oldId)).toEqual([]);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);

    await (evm.actor as any).set_withdrawal(withdrawal(minimum));
    expect(await (bridge.actor as any).notify_withdrawal({
      transaction_hash: new Uint8Array(32).fill(9),
    })).toHaveProperty("Err.RateLimited");
    expect(await (bridge.actor as any).notify_withdrawal({
      transaction_hash: new Uint8Array(32).fill(10),
    })).toHaveProperty("Ok.Ingested");
    expect((await (bridge.actor as any).get_bridge_status()).counts.withdrawals).toBe(1n);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
  }

  it(
    "rejects a pre-boundary withdrawal before state or Ledger effects and accepts the boundary",
    rejects_a_pre_boundary_withdrawal_before_state_or_Ledger_effects_and_accepts_the_boundary,
  );

  it("charges ingestion quota only when the validated withdrawal is committed", async () => {
    const { evm, bridge, runtimePrincipal } = await setup(true, {
      notification_ingestion_rate_limit_global: 1,
    });
    const id = new Uint8Array(32).fill(0x68);
    await (evm.actor as any).set_withdrawal([{
      id,
      owner: runtimePrincipal.toUint8Array(),
      subaccount: new Uint8Array(32),
      amount: 1_000_000n,
      max_service_fee: 100_000n,
      charged_service_fee: 100_000n,
      amount_out: 900_000n,
    }]);
    await (evm.actor as any).set_receipt_mode({ DecodeFailure: null });
    expect(await (bridge.actor as any).notify_withdrawal({
      transaction_hash: new Uint8Array(32).fill(0x67),
    })).toEqual({ Err: { InvalidBaseResponse: null } });

    await (evm.actor as any).set_receipt_mode({ Confirmed: null });
    expect(await (bridge.actor as any).notify_withdrawal({
      transaction_hash: new Uint8Array(32).fill(9),
    })).toHaveProperty("Ok.Ingested");
    expect(await (bridge.actor as any).get_withdrawal(id)).toHaveLength(1);
  });

  it("never calls the Ledger before the user withdrawal reaches the finalized head", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xa0);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 1_000_000n, max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n }]);
    await (evm.actor as any).set_finalized_block_sequence([98n]);
    const premature: any = await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    expect(premature).toHaveProperty("Err.TransactionNotConfirmed");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    await (evm.actor as any).set_finalized_block_sequence([100n]);
    const notified: any = await notifyFixtureWithdrawal(bridge, new Uint8Array(32).fill(10));
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
      await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 1_000_000n, max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n }]);
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
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const pinnedCallsBefore = Array.from(
      await (evm.actor as any).pinned_eth_call_block_numbers(),
    );
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
      amount: 1_000_000n,
      max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n,
    }]);

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) }))
      .toHaveProperty("Ok.Ingested");
    expect(Array.from(await (evm.actor as any).pinned_eth_call_block_numbers())).toEqual([
      ...pinnedCallsBefore,
      99n,
      100n,
      100n,
    ]);
  });

  async function runtime_attestation_is_reused_across_withdrawal_upgrade_and_governance() {
    const { evm, bridge, init, runtimePrincipal } = await setup(false);
    // Operational sealing performs the initial attestation and caches it for
    // withdrawal and governance paths until the deployment is reinstalled.
    const initialAttestationCalls = await (evm.actor as any).get_code_call_count();
    expect(initialAttestationCalls).toBe(3n);
    const id = new Uint8Array(32).fill(0x9b);
    await (evm.actor as any).set_withdrawal([{
      id,
      owner: runtimePrincipal.toUint8Array(),
      subaccount: new Uint8Array(32),
      amount: 1_000_000n,
      max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n,
    }]);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) }))
      .toHaveProperty("Ok.Ingested");
    expect(await (evm.actor as any).chain_id_call_count()).toBe(0n);
    expect(await (evm.actor as any).get_code_call_count()).toBe(initialAttestationCalls);

    const secondId = new Uint8Array(32).fill(0x9c);
    await (evm.actor as any).set_observed_transaction(
      new Uint8Array(32).fill(10),
      new Uint8Array(20).fill(1),
      new Uint8Array(20).fill(0x22),
      100n,
    );
    await (evm.actor as any).set_withdrawal([{
      id: secondId,
      owner: runtimePrincipal.toUint8Array(),
      subaccount: new Uint8Array(32),
      amount: 1_000_000n,
      max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n,
    }]);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(10) }))
      .toHaveProperty("Ok.Ingested");
    expect(await (evm.actor as any).get_code_call_count()).toBe(initialAttestationCalls);

    await pic!.upgradeCanister({
      canisterId: bridge.canisterId,
      wasm: readFileSync(bridgeWasm),
      arg: IDL.encode([], []),
    });
    bridge.actor.setPrincipal(runtimePrincipal);
    expect(await (bridge.actor as any).prepare_base_governance_action({
      SetServiceFee: { value: 1n },
    })).toHaveProperty("Ok.chain_id", 8453n);
    expect(await (evm.actor as any).get_code_call_count()).toBe(initialAttestationCalls);
    expect(await (evm.actor as any).chain_id_call_count()).toBe(0n);

    await pic!.reinstallCode({
      canisterId: bridge.canisterId,
      wasm: readFileSync(bridgeWasm),
      arg: IDL.encode([bridgeInit], [init]),
    });
    bridge.actor.setPrincipal(Principal.anonymous());
    expect(await (bridge.actor as any).initialize_public_config()).toHaveProperty("Ok");
    bridge.actor.setPrincipal(runtimePrincipal);
    expect((await (bridge.actor as any).get_bridge_status()).deposits_paused).toBe(true);
    expect(await (evm.actor as any).get_code_call_count()).toBe(initialAttestationCalls);
    expect(await (bridge.actor as any).seal_operational_config({
      governance_evm_fee: init.governance_evm_fee,
      cycles_floor: init.cycles_floor,
      settlement_cycle_ceiling: init.settlement_cycle_ceiling,
    })).toHaveProperty("Ok.lifecycle.OperationalConfigSealed");
    const reinstalledAttestationCalls = initialAttestationCalls + 3n;
    expect(await (evm.actor as any).get_code_call_count()).toBe(reinstalledAttestationCalls);
    expect(await (bridge.actor as any).prepare_base_governance_action({
      SetServiceFee: { value: 1n },
    })).toHaveProperty("Ok.chain_id", 8453n);
    expect(await (evm.actor as any).get_code_call_count()).toBe(reinstalledAttestationCalls);
    expect(await (evm.actor as any).chain_id_call_count()).toBe(0n);
  }

  it(
    "reuses runtime attestation across withdrawal and upgrade but not reinstall",
    runtime_attestation_is_reused_across_withdrawal_upgrade_and_governance,
  );

  it("allows only Governance or the fixed confirmation relayer to refresh activation evidence", async () => {
    const { bridge, runtimePrincipal, confirmationRelayerPrincipal } = await setup(false);
    bridge.actor.setPrincipal(Principal.anonymous());
    expect(await (bridge.actor as any).refresh_activation_attestation())
      .toHaveProperty("Err.Unauthorized");
    await pic!.advanceTime(31_000);
    bridge.actor.setPrincipal(confirmationRelayerPrincipal);
    expect(await (bridge.actor as any).refresh_activation_attestation())
      .toHaveProperty("Ok.deposits_paused", true);
    await pic!.advanceTime(31_000);
    bridge.actor.setPrincipal(runtimePrincipal);
    expect(await (bridge.actor as any).refresh_activation_attestation())
      .toHaveProperty("Ok.deposits_paused", true);
  });

  it.each([
    { mode: { FinalizedUnavailable: null }, error: "RpcUnavailable", tag: 0x9c },
    { mode: { CanonicalInconsistent: null }, error: "RpcInconsistent", tag: 0x9f },
    { mode: { SameHeightDifferentHash: null }, error: "InvalidBaseResponse", tag: 0x9d },
  ])("fails closed on $error canonical block observations before Ledger", async ({ mode, error, tag }) => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(tag);
    await (evm.actor as any).set_withdrawal([{
      id,
      owner: runtimePrincipal.toUint8Array(),
      subaccount: new Uint8Array(32),
      amount: 1_000_000n,
      max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n,
    }]);
    await (evm.actor as any).set_block_mode(mode);

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) }))
      .toHaveProperty(`Err.${error}`);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  });

  async function accepts_staggered_finalized_heads_only_after_an_exact_checkpoint_quorum() {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0x9e);
    await (evm.actor as any).set_withdrawal([{
      id,
      owner: runtimePrincipal.toUint8Array(),
      subaccount: new Uint8Array(32),
      amount: 1_000_000n,
      max_service_fee: 100_000n,
      charged_service_fee: 100_000n,
      amount_out: 900_000n,
    }]);
    await (evm.actor as any).set_observed_transaction(
      new Uint8Array(32).fill(9),
      new Uint8Array(20).fill(1),
      new Uint8Array(20).fill(0x22),
      101n,
    );
    await (evm.actor as any).set_finalized_block_sequence([100n, 101n, 102n]);
    await (evm.actor as any).set_block_mode({ FinalizedInconsistent: null });

    const result: any = await (bridge.actor as any).notify_withdrawal({
      transaction_hash: new Uint8Array(32).fill(9),
    });

    expect(result).toHaveProperty("Ok.Ingested.finalized_checkpoint_block_number", 101n);
    expect(await (bridge.actor as any).get_withdrawal(id)).toHaveLength(1);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
  }

  it(
    "accepts staggered finalized heads only after an exact checkpoint quorum",
    accepts_staggered_finalized_heads_only_after_an_exact_checkpoint_quorum,
  );

  async function rejects_a_checkpoint_hash_quorum_that_depends_on_a_provider_below_the_checkpoint() {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xae);
    await (evm.actor as any).set_withdrawal([{
      id,
      owner: runtimePrincipal.toUint8Array(),
      subaccount: new Uint8Array(32),
      amount: 1_000_000n,
      max_service_fee: 100_000n,
      charged_service_fee: 100_000n,
      amount_out: 900_000n,
    }]);
    await (evm.actor as any).set_observed_transaction(
      new Uint8Array(32).fill(9),
      new Uint8Array(20).fill(1),
      new Uint8Array(20).fill(0x22),
      100n,
    );
    await (evm.actor as any).set_finalized_block_sequence([90n, 100n, 110n]);
    await (evm.actor as any).set_block_mode({ FinalizedCheckpointFork: null });

    expect(await (bridge.actor as any).notify_withdrawal({
      transaction_hash: new Uint8Array(32).fill(9),
    })).toHaveProperty("Err.RpcInconsistent");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  }

  it(
    "rejects a checkpoint hash quorum that depends on a provider below the checkpoint",
    rejects_a_checkpoint_hash_quorum_that_depends_on_a_provider_below_the_checkpoint,
  );

  async function rejects_a_receipt_above_the_two_provider_finalized_checkpoint() {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0x9f);
    await (evm.actor as any).set_withdrawal([{
      id,
      owner: runtimePrincipal.toUint8Array(),
      subaccount: new Uint8Array(32),
      amount: 1_000_000n,
      max_service_fee: 100_000n,
      charged_service_fee: 100_000n,
      amount_out: 900_000n,
    }]);
    await (evm.actor as any).set_observed_transaction(
      new Uint8Array(32).fill(9),
      new Uint8Array(20).fill(1),
      new Uint8Array(20).fill(0x22),
      102n,
    );
    await (evm.actor as any).set_finalized_block_sequence([101n, 102n, 101n]);
    await (evm.actor as any).set_block_mode({ FinalizedInconsistent: null });

    expect(await (bridge.actor as any).notify_withdrawal({
      transaction_hash: new Uint8Array(32).fill(9),
    })).toHaveProperty("Err.TransactionNotConfirmed");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  }

  it(
    "rejects a receipt above the two provider finalized checkpoint",
    rejects_a_receipt_above_the_two_provider_finalized_checkpoint,
  );

  it("rejects a non-committed old receipt before any Ledger release call", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xa1);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 1_000_000n, max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n }]);
    await (evm.actor as any).set_withdrawal_status(0);

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toEqual({ Err: { BaseStateMismatch: null } });
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  });

  it("rejects signer rotation between the receipt and finalized Base state read before Ledger", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xa2);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 1_000_000n, max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n }]);
    expect(await (evm.actor as any).set_bridge_signer(new Uint8Array(20).fill(0xaa))).toHaveProperty("Ok");

    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toEqual({ Err: { BridgeSignerMismatch: null } });
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    expect(await (bridge.actor as any).get_withdrawal(id)).toEqual([]);
  });

  async function allows_a_non_owner_relayer_while_keeping_the_Ledger_recipient_bound_to_the_event() {
    const { ledger, evm, bridge } = await setup();
    const id = new Uint8Array(32).fill(86);
    const owner = Principal.selfAuthenticating(new Uint8Array(32).fill(8));
    const relayer = Principal.selfAuthenticating(new Uint8Array(32).fill(9));
    const subaccount = new Uint8Array(32).fill(0x4a);
    await (evm.actor as any).set_withdrawal([{ id, owner: owner.toUint8Array(), subaccount, amount: 1_000_000n, max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n }]);
    bridge.actor.setPrincipal(Principal.anonymous());
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) }))
      .toEqual({ Err: { AnonymousCaller: null } });
    bridge.actor.setPrincipal(relayer);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Ok.Ingested");
    expect(await (bridge.actor as any).get_withdrawal(id)).toHaveLength(1);
    bridge.actor.setPrincipal(Principal.anonymous());
    expect(await (bridge.actor as any).continue_withdrawal(id))
      .toEqual({ Err: { AnonymousCaller: null } });
    bridge.actor.setPrincipal(relayer);
    expect(await continueWithdrawal(bridge, id)).toHaveProperty("Ok.Complete");
    const transfer = (await (ledger.actor as any).ledger_transactions()).at(-1)?.transfer?.[0];
    expect(transfer.to.owner.toText()).toBe(owner.toText());
    expect(Array.from(transfer.to.subaccount[0])).toEqual(Array.from(subaccount));
    expect(transfer.to.owner.toText()).not.toBe(relayer.toText());
  }

  it(
    "allows a non-owner relayer while keeping the Ledger recipient bound to the event",
    allows_a_non_owner_relayer_while_keeping_the_Ledger_recipient_bound_to_the_event,
  );

  it("rejects non-confirmed notifications and ingests one concurrent replay", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(86);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 1_000_000n, max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n }]);
    await (evm.actor as any).set_observed_transaction(new Uint8Array(32).fill(9), new Uint8Array(20).fill(1), new Uint8Array(20).fill(0x22), 101n);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Err.TransactionNotConfirmed");
    await (evm.actor as any).set_observed_transaction(new Uint8Array(32).fill(10), new Uint8Array(20).fill(1), new Uint8Array(20).fill(0x22), 99n);
    const deferred = pic!.createDeferredActor(bridgeIdl, bridge.canisterId) as any;
    deferred.setPrincipal(runtimePrincipal);
    const first = await deferred.notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(10) });
    const second = await deferred.notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(10) });
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
      transaction_hash: new Uint8Array(32).fill(10),
    })).toHaveProperty("Ok.Duplicate");
  });

  it("returns the canonical duplicate for a known notification hash without re-reading RPC", async () => {
    const { evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(89);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 1_000_000n, max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n }]);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Ok.Ingested");
    const callsAfterIngest = await (evm.actor as any).receipt_call_count();
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 1_000_001n, max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n }]);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Ok.Duplicate");
    expect(await (evm.actor as any).receipt_call_count()).toBe(callsAfterIngest);
  });

  it("rate limits a caller after six distinct unknown hashes before further RPC", async () => {
    const { evm, bridge } = await setup();
    const callsBefore = await (evm.actor as any).receipt_call_count();
    for (let attempt = 0; attempt < 6; attempt += 1) {
      expect(await (bridge.actor as any).notify_withdrawal({
        transaction_hash: new Uint8Array(32).fill(0xa0 + attempt),
      })).toHaveProperty("Err.InvalidBaseResponse");
    }
    expect(await (evm.actor as any).receipt_call_count()).toBe(callsBefore + 6n);
    expect(await (bridge.actor as any).notify_withdrawal({
      transaction_hash: new Uint8Array(32).fill(0xa6),
    })).toHaveProperty("Err.RateLimited");
    expect(await (evm.actor as any).receipt_call_count()).toBe(callsBefore + 6n);
  });

  it("uses the fixed fee without querying Ledger fee availability", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(87);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 1_000_000n, max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n }]);
    await (ledger.actor as any).set_ledger_fee_available(false);
    expect(await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) })).toHaveProperty("Ok.Ingested");
    const paid: any = await (bridge.actor as any).get_withdrawal(id);
    expect(paid[0].ledger_fee).toBe(testLedgerFee);
  });

  it("fails closed when the charged service fee is below the fixed Ledger fee", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xb7);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 10_000n, max_service_fee: 9_999n, charged_service_fee: 9_999n, amount_out: 1n }]);

    const guarded: any = await (bridge.actor as any).notify_withdrawal({ transaction_hash: new Uint8Array(32).fill(9) });
    expect(guarded).toEqual({ Err: { LedgerFeeExceedsServiceFee: { ledger_fee: testLedgerFee, charged_service_fee: 9_999n } } });
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(0n);
    const blocked: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(blocked[0].state)).toBe("Observed");
    expect(blocked[0].last_settlement_stop_reason).toEqual([{ LedgerFeeExceedsServiceFee: null }]);
    expect((await (bridge.actor as any).get_bridge_status()).withdrawal_fee_guard_active).toBe(true);
  });

  it("continues an ambiguous Withdrawal release from reconciled Hold to Paid", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    const id = new Uint8Array(32).fill(46);
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 1_000_000n, max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n }]);
    await notifyFixtureWithdrawal(bridge);
    const held: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(held[0].state)).toBe("ReconciliationHold");
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    await advancePastReconciliationDelay();
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
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 1_000_000n, max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n }]);
    await notifyFixtureWithdrawal(bridge);

    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    const stopped: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(stopped[0].state)).toBe("ReleasePending");
    expect(stopped[0].ledger_fee).toBe(testLedgerFee);
    expect(stopped[0].last_settlement_stop_reason[0].LedgerRejected).toContain("BadFee");

    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    expect(await continueWithdrawal(bridge, id)).toHaveProperty("Ok.Complete");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(2n);
    const paid: any = await (bridge.actor as any).get_withdrawal(id);
    expect(phaseName(paid[0].state)).toBe("Paid");
    expect(paid[0].ledger_fee).toBe(testLedgerFee);
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });


  it("does not reprice or cancel after an ambiguous Ledger release", async () => {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const id = new Uint8Array(32).fill(0xb5);
    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    await (evm.actor as any).set_withdrawal([{ id, owner: runtimePrincipal.toUint8Array(), subaccount: new Uint8Array(32), amount: 1_000_000n, max_service_fee: 100_000n, charged_service_fee: 100_000n, amount_out: 900_000n }]);
    await notifyFixtureWithdrawal(bridge);
    expect(phaseName((await (bridge.actor as any).get_withdrawal(id))[0].state)).toBe("ReconciliationHold");

    await (ledger.actor as any).set_ledger_fee(2n);
    await (ledger.actor as any).set_ledger_mode({ BadFee: null });
    await advancePastReconciliationDelay();
    const retry: any = await continueWithdrawal(bridge, id);
    expect(retry).toHaveProperty("Ok.Stopped.reason.LedgerRejected");
    expect(phaseName((await (bridge.actor as any).get_withdrawal(id))[0].state)).toBe("ReconciliationHold");
  });


  it("continues an ambiguous deposit from reconciled Hold to a signed Mint Authorization", async () => {
    const { ledger, evm, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    const result: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n });
    expect(phaseName(result.Ok.state)).toBe("FundingReconciliationHold");
    expect(phaseName((await (bridge.actor as any).get_deposit(result.Ok.deposit_id))[0].state)).toBe("FundingReconciliationHold");
    const before: any = await (bridge.actor as any).get_bridge_status();
    await pic!.upgradeCanister({ canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm), arg: IDL.encode([], []) });
    const after: any = await (bridge.actor as any).get_bridge_status();
    expect(after.counts.reconciliation_holds).toBe(before.counts.reconciliation_holds);
    expect(after.counts.reconciliation_holds).toBe(1n);
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    await advancePastReconciliationDelay();
    // The reconciliation delay exceeds the authorization TTL. Model a fresh
    // finalized Base block so recovery cannot revive an already-expired quote.
    await (evm.actor as any).set_block_timestamp(BigInt(Math.floor((await pic!.getTime()) / 1_000)));
    await mintAuthorizedDeposit(bridge, evm, result.Ok.deposit_id);
    const stored: any = await (bridge.actor as any).get_deposit(result.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("Minted");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("retains one retryable funding identity without repeating early preflight or quota", async () => {
    const { ledger, evm, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode({ TemporarilyUnavailable: null });

    const request = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n };
    const baseCallsBefore = await (evm.actor as any).deposit_processed_call_count();
    const result: any = await (bridge.actor as any).request_deposit(request);

    expect(result).toHaveProperty("Err.FundingUnavailable.retry_after_seconds", 30n);
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(baseCallsBefore);
    expect(await (bridge.actor as any).request_deposit(request)).toHaveProperty("Err.FundingUnavailable");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(1n);
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(baseCallsBefore);
    await pic!.advanceTime(31_000);
    await pic!.tick(5);
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    const funded: any = await (bridge.actor as any).request_deposit(request);
    expect(funded).toHaveProperty("Ok.state.EscrowedUnquoted");
    expect(await (ledger.actor as any).ledger_transfer_calls()).toBe(2n);
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(baseCallsBefore + 1n);

    const next = (owner_sequence: bigint) => ({ ...request, owner_sequence });
    expect(await (bridge.actor as any).request_deposit(next(1n))).toHaveProperty("Ok");
    expect(await (bridge.actor as any).request_deposit(next(2n))).toHaveProperty("Ok");
    expect(await (bridge.actor as any).request_deposit(next(3n))).toHaveProperty("Err.RateLimited");
  });

  async function refund_observation_preserves_global_runtime_attestation() {
    const { ledger, evm, bridge, runtimePrincipal } = await setup();
    const activationAttestationCalls = await (evm.actor as any).get_code_call_count();
    expect(activationAttestationCalls).toBeGreaterThanOrEqual(3n);
    const result: any = await requestDefaultDeposit(bridge);
    expect(await (evm.actor as any).get_code_call_count()).toBe(activationAttestationCalls);
    await assertAuthorizationRefunded(bridge, evm, result.Ok.deposit_id);
    expect((await (ledger.actor as any).ledger_transactions())).toHaveLength(2);
    expect(await (evm.actor as any).get_code_call_count()).toBe(activationAttestationCalls);

    const withdrawalId = new Uint8Array(32).fill(0x8b);
    await (evm.actor as any).set_withdrawal([{
      id: withdrawalId,
      owner: runtimePrincipal.toUint8Array(),
      subaccount: new Uint8Array(32),
      amount: 1_000_000n,
      max_service_fee: 100_000n,
      charged_service_fee: 100_000n,
      amount_out: 900_000n,
    }]);
    expect(await (bridge.actor as any).notify_withdrawal({
      transaction_hash: new Uint8Array(32).fill(9),
    })).toHaveProperty("Ok.Ingested");
    expect(await (evm.actor as any).get_code_call_count()).toBe(activationAttestationCalls);
    expect(await (evm.actor as any).chain_id_call_count()).toBe(0n);
  }

  it(
    "persists refund observation for cross-path runtime attestation reuse",
    refund_observation_preserves_global_runtime_attestation,
  );

  it("has no success confirmation API and persists exact Mint evidence only during a refund claim", async () => {
    const { evm, bridge } = await setup();
    const result: any = await requestDefaultDeposit(bridge);
    const authorization = await awaitMintAuthorization(bridge, result.Ok.deposit_id);
    const transactionHash = new Uint8Array(32).fill(0x52);
    await evm.actor.set_observed_transaction(
      transactionHash,
      authorization.verifying_contract,
      new Uint8Array(20).fill(0x77),
      authorization.finalized_block_number,
    );
    await evm.actor.set_mint_log([{
      deposit_id: authorization.deposit_id,
      recipient: authorization.recipient,
      authorization_digest: authorization.digest,
      gross_amount: authorization.gross_amount,
      charged_service_fee: authorization.charged_service_fee,
      minted_amount: authorization.gross_amount - authorization.charged_service_fee,
      transaction_hash: transactionHash,
    }]);
    expect((bridge.actor as any).notify_deposit_mint).toBeUndefined();
    expect(phaseName((await bridge.actor.get_deposit(result.Ok.deposit_id))[0].state)).toBe("AuthorizationAvailable");
    await evm.actor.set_processed_deposit(true);
    await setExpiredBlockTimestamp(evm, authorization.deadline + 1n);
    expect(await (bridge.actor as any).request_deposit_refund(result.Ok.deposit_id))
      .toEqual({ Err: { NotClaimable: null } });
    expect(phaseName((await bridge.actor.get_deposit(result.Ok.deposit_id))[0].state)).toBe("Minted");
  });

  async function fails_closed_when_processed_is_true_but_exact_Mint_evidence_is_missing() {
    const { ledger, evm, bridge } = await setup();
    const result: any = await requestDefaultDeposit(bridge);
    const authorization = await awaitMintAuthorization(bridge, result.Ok.deposit_id);
    await evm.actor.set_processed_deposit(true);
    await evm.actor.set_mint_log([]);
    await setExpiredBlockTimestamp(evm, authorization.deadline + 1n);
    const processedCallsBeforeExpiry = await (evm.actor as any).deposit_processed_call_count();
    expect(await (bridge.actor as any).request_deposit_refund(result.Ok.deposit_id)).toHaveProperty("Err");
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(processedCallsBeforeExpiry + 1n);
    const stored: any = await bridge.actor.get_deposit(result.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("RefundAvailable");
    expect(stored[0].refund).toEqual([]);
    expect(await (ledger.actor as any).ledger_transactions()).toHaveLength(1);

    const processedCallsBeforeUpgradeRetry = await (evm.actor as any).deposit_processed_call_count();
    await pic!.upgradeCanister({
      canisterId: bridge.canisterId,
      wasm: readFileSync(bridgeWasm),
      arg: IDL.encode([], []),
    });
    expect(await (bridge.actor as any).request_deposit_refund(result.Ok.deposit_id)).toHaveProperty("Err");
    expect(await (evm.actor as any).deposit_processed_call_count()).toBe(processedCallsBeforeUpgradeRetry + 1n);
    expect(phaseName((await bridge.actor.get_deposit(result.Ok.deposit_id))[0].state)).toBe("RefundAvailable");
    expect(await (ledger.actor as any).ledger_transactions()).toHaveLength(1);
  }

  it(
    "fails closed when processed is true but exact Mint evidence is missing",
    fails_closed_when_processed_is_true_but_exact_Mint_evidence_is_missing,
  );

  it("classifies exact Mint RPC failures and audits provider disagreement", async () => {
    const { evm, bridge } = await setup();
    const result: any = await requestDefaultDeposit(bridge);
    const authorization = await awaitMintAuthorization(bridge, result.Ok.deposit_id);
    const transactionHash = new Uint8Array(32).fill(0x53);
    await evm.actor.set_observed_transaction(
      transactionHash,
      authorization.verifying_contract,
      new Uint8Array(20).fill(0x77),
      authorization.finalized_block_number,
    );
    await evm.actor.set_mint_log([{
      deposit_id: authorization.deposit_id,
      recipient: authorization.recipient,
      authorization_digest: authorization.digest,
      gross_amount: authorization.gross_amount,
      charged_service_fee: authorization.charged_service_fee,
      minted_amount: authorization.gross_amount - authorization.charged_service_fee,
      transaction_hash: transactionHash,
    }]);
    await evm.actor.set_processed_deposit(true);
    await setExpiredBlockTimestamp(evm, authorization.deadline + 1n);

    await (evm.actor as any).set_receipt_mode({ RpcFailure: null });
    expect(await (bridge.actor as any).request_deposit_refund(result.Ok.deposit_id))
      .toEqual({ Err: { FinalityUnavailable: null } });
    await (evm.actor as any).set_receipt_mode({ Reverted: null });
    expect(await (bridge.actor as any).request_deposit_refund(result.Ok.deposit_id))
      .toEqual({ Err: { BaseStateMismatch: null } });
    await (evm.actor as any).set_receipt_mode({ Inconsistent: null });
    expect(await (bridge.actor as any).request_deposit_refund(result.Ok.deposit_id))
      .toEqual({ Err: { RpcInconsistent: null } });

    const stored: any = await bridge.actor.get_deposit(result.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("RefundAvailable");
    expect(stored[0].refund).toEqual([]);
    const audit: any = await (bridge.actor as any).get_audit_events(0n, 100);
    expect(audit.Ok.events.some((event: any) =>
      event.kind.EvmRpcDecision?.operation === "request_deposit_refund_exact_mint"
    )).toBe(true);
  });

  async function processed_id_with_another_digest_cannot_move_refund_funds() {
    const { ledger, evm, bridge } = await setup();
    const result: any = await requestDefaultDeposit(bridge);
    const authorization = await awaitMintAuthorization(bridge, result.Ok.deposit_id);
    const transactionHash = new Uint8Array(32).fill(0x54);
    await evm.actor.set_observed_transaction(
      transactionHash,
      authorization.verifying_contract,
      new Uint8Array(20).fill(0x77),
      authorization.finalized_block_number,
    );
    await evm.actor.set_mint_log([{
      deposit_id: authorization.deposit_id,
      recipient: authorization.recipient,
      authorization_digest: new Uint8Array(32).fill(0xff),
      gross_amount: authorization.gross_amount,
      charged_service_fee: authorization.charged_service_fee,
      minted_amount: authorization.gross_amount - authorization.charged_service_fee,
      transaction_hash: transactionHash,
    }]);
    await evm.actor.set_processed_deposit(true);
    await setExpiredBlockTimestamp(evm, authorization.deadline + 1n);

    expect(await (bridge.actor as any).request_deposit_refund(result.Ok.deposit_id))
      .toEqual({ Err: { DepositIdentityConflict: null } });
    expect(await (ledger.actor as any).ledger_transactions()).toHaveLength(1);
    const stored: any = await bridge.actor.get_deposit(result.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("RefundAvailable");
    expect(stored[0].refund).toEqual([]);
  }

  it("keeps funds fixed when a processed Deposit ID has only another authorization digest", processed_id_with_another_digest_cannot_move_refund_funds);

  it("does not refund when finalized Base RPC observations cannot prove an exact checkpoint quorum", async () => {
    const { evm, bridge } = await setup();
    const result: any = await requestDefaultDeposit(bridge);
    const authorization = await awaitMintAuthorization(bridge, result.Ok.deposit_id);
    await setExpiredBlockTimestamp(evm, authorization.deadline + 1n);
    await evm.actor.set_finalized_block_sequence([90n, 100n, 110n]);
    await evm.actor.set_block_mode({ FinalizedCheckpointFork: null });
    expect(await (bridge.actor as any).request_deposit_refund(result.Ok.deposit_id))
      .toEqual({ Err: { RpcInconsistent: null } });
    const stored: any = await bridge.actor.get_deposit(result.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("AuthorizationAvailable");
    expect(stored[0].refund).toEqual([]);
    const audit: any = await (bridge.actor as any).get_audit_events(0n, 100);
    expect(audit.Ok.events.some((event: any) =>
      event.kind.EvmRpcDecision?.operation === "request_deposit_refund_recovery"
    )).toBe(true);
  });

  async function deposit_refund_rejects_the_withdrawal_checkpoint_exception() {
    const { evm, bridge } = await setup();
    const result: any = await requestDefaultDeposit(bridge);
    const authorization = await awaitMintAuthorization(bridge, result.Ok.deposit_id);
    await setExpiredBlockTimestamp(evm, authorization.deadline + 1n);
    await evm.actor.set_finalized_block_sequence([100n, 101n, 102n]);
    await evm.actor.set_block_mode({ FinalizedInconsistent: null });

    expect(await (bridge.actor as any).request_deposit_refund(result.Ok.deposit_id))
      .toEqual({ Err: { RpcInconsistent: null } });
    const stored: any = await bridge.actor.get_deposit(result.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("AuthorizationAvailable");
    expect(stored[0].refund).toEqual([]);
  }

  it(
    "does not apply the withdrawal checkpoint exception to Deposit refunds",
    deposit_refund_rejects_the_withdrawal_checkpoint_exception,
  );

  it("persists the public notification budget across upgrade and protects six recovery slots", async () => {
    const { evm, bridge } = await setup();
    const callsBefore = await (evm.actor as any).receipt_call_count();
    for (let index = 0; index < 54; index += 1) {
      bridge.actor.setPrincipal(
        Principal.selfAuthenticating(new Uint8Array(32).fill(Math.floor(index / 6) + 200)),
      );
      const rejected: any = await (bridge.actor as any).notify_withdrawal({
        transaction_hash: new Uint8Array(32).fill(index + 100),
      });
      expect(rejected).toHaveProperty("Err.InvalidBaseResponse");
    }
    expect(await (evm.actor as any).receipt_call_count()).toBe(callsBefore + 54n);

    bridge.actor.setPrincipal(Principal.selfAuthenticating(new Uint8Array(32).fill(250)));
    expect(await (bridge.actor as any).notify_withdrawal({
      transaction_hash: new Uint8Array(32).fill(0xf0),
    })).toHaveProperty("Err.RateLimited");
    expect(await (evm.actor as any).receipt_call_count()).toBe(callsBefore + 54n);

    await pic!.upgradeCanister({
      canisterId: bridge.canisterId,
      wasm: readFileSync(bridgeWasm),
      arg: IDL.encode([], []),
    });
    bridge.actor.setPrincipal(Principal.selfAuthenticating(new Uint8Array(32).fill(251)));
    expect(await (bridge.actor as any).notify_withdrawal({
      transaction_hash: new Uint8Array(32).fill(0xf1),
    })).toHaveProperty("Err.RateLimited");
    expect(await (evm.actor as any).receipt_call_count()).toBe(callsBefore + 54n);

    await pic!.advanceTime(10 * 60_000 + 1);
    await pic!.tick(5);
    bridge.actor.setPrincipal(Principal.selfAuthenticating(new Uint8Array(32).fill(252)));
    expect(await (bridge.actor as any).notify_withdrawal({
      transaction_hash: new Uint8Array(32).fill(0xf2),
    })).toHaveProperty("Err.InvalidBaseResponse");
    expect(await (evm.actor as any).receipt_call_count()).toBe(callsBefore + 55n);
  });

  it("keeps retryable funding outside the public settlement quota", async () => {
    const { ledger, bridge } = await setup();
    await (ledger.actor as any).set_ledger_mode({ TemporarilyUnavailable: null });
    const request = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n };
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
    expect(await (bridge.actor as any).request_deposit_refund(result.Ok.deposit_id))
      .toEqual({ Err: { NotClaimable: null } });
    let stored: any = await bridge.actor.get_deposit(result.Ok.deposit_id);
    expect(stored[0].refund).toEqual([]);

    await setExpiredBlockTimestamp(evm, authorization.deadline + 1n);
    expect(await (bridge.actor as any).request_deposit_refund(result.Ok.deposit_id)).toHaveProperty("Ok.state.Refunded");
    stored = await bridge.actor.get_deposit(result.Ok.deposit_id);
    expect(phaseName(stored[0].state)).toBe("Refunded");
  });



  async function activation_is_only_resume_path() {
    const { evm, bridge, init, runtimePrincipal } = await setup();
    await (evm.actor as any).set_max_service_fee(250_000n);
    await (evm.actor as any).set_service_fee(250_000n);
    bridge.actor.setPrincipal(init.pause_principal);
    expect(await (bridge.actor as any).pause_new_deposits()).toHaveProperty("Ok");
    bridge.actor.setPrincipal(runtimePrincipal);
    const args = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 900_000n, max_service_fee: 250_000n };
    expect(await (bridge.actor as any).request_deposit(args)).toEqual({ Err: { DepositsPaused: null } });
    await activateBridgeThroughGovernance(bridge, evm, runtimePrincipal);
    const deposit: any = await (bridge.actor as any).request_deposit(args);
    expect(deposit).toHaveProperty("Ok");
    const audit: any = await (bridge.actor as any).get_audit_events(0n, 100);
    expect(audit.Ok.events.length).toBeGreaterThanOrEqual(2);
    expect(await (bridge.actor as any).request_fee_payout(1n)).toEqual({ Err: { InsufficientFeeReserve: null } });
    await awaitMintAuthorization(bridge, deposit.Ok.deposit_id);
    expect(await (bridge.actor as any).request_fee_payout(1n)).toHaveProperty("Ok");
    await mintAuthorizedDeposit(bridge, evm, deposit.Ok.deposit_id);
    expect(phaseName((await (bridge.actor as any).get_deposit(deposit.Ok.deposit_id))[0].state)).toBe("Minted");
    expect(await (bridge.actor as any).request_fee_payout(1n)).toHaveProperty("Ok");
  }

  it(
    "resumes local deposits only after the confirmed activation path",
    activation_is_only_resume_path,
  );

  it("installs with new deposits paused until Governance activates them", async () => {
    const { bridge } = await setup(false);
    expect((await (bridge.actor as any).get_bridge_status()).deposits_paused).toBe(true);
    const args = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n };
    expect(await (bridge.actor as any).request_deposit(args)).toEqual({ Err: { DepositsPaused: null } });
  });

  it("keeps Mint gas outside Deposit reserve admission", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_eth_balance(0n);
    const args = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n };
    const result: any = await (bridge.actor as any).request_deposit(args);
    expect(result).toHaveProperty("Ok.state.EscrowedUnquoted");
    await awaitMintAuthorization(bridge, result.Ok.deposit_id);
    expect(phaseName((await (bridge.actor as any).get_deposit(result.Ok.deposit_id))[0].state)).toBe("AuthorizationAvailable");
    expect((await (ledger.actor as any).ledger_transactions()).length).toBe(1);
  });

  it("rejects a definitive Ledger pull failure without creating formal deposit artifacts", async () => {
    const { ledger, bridge, runtimePrincipal } = await setup();
    const failed = { owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 200_000n, max_service_fee: 10n };
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
      gross_amount: 200_000n,
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
      gross_amount: 200_000n,
      max_service_fee: 10n,
    });
    await advanceDepositJobs(bridge, deposited.Ok.deposit_id);
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
      gross_amount: 200_000n,
      max_service_fee: 10n,
    });
    expect(first).toHaveProperty("Ok");
    await advanceDepositJobs(bridge, first.Ok.deposit_id);
    expect(await (ledger.actor as any).ledger_transactions()).toHaveLength(1);

    await (ledger.actor as any).set_ledger_mode({ Trap: null });
    const ambiguous: any = await (bridge.actor as any).request_deposit({
      owner_sequence: 1n,
      base_recipient: new Uint8Array(20).fill(5),
      from_subaccount: [],
      gross_amount: 200_000n,
      max_service_fee: 10n,
    });
    await advanceDepositJobs(bridge, ambiguous.Ok.deposit_id);
    expect(phaseName((await (bridge.actor as any).get_deposit(ambiguous.Ok.deposit_id))[0].state)).toBe("FundingReconciliationHold");

    await (index.actor as any).set_index_synced_blocks([0n]);
    await pic!.advanceTime(24 * 60 * 60 * 1_000 + 1);
    await (ledger.actor as any).set_ledger_mode({ Succeed: null });
    await advanceDepositJobs(bridge, ambiguous.Ok.deposit_id);
    expect(phaseName((await (bridge.actor as any).get_deposit(ambiguous.Ok.deposit_id))[0].state)).toBe(
      "FundingReconciliationHold",
    );
    expect(await (ledger.actor as any).ledger_transactions()).toHaveLength(1);

    await (index.actor as any).set_index_synced_blocks([1n]);
    await advancePastReconciliationDelay();
    await advanceDepositJobs(bridge, ambiguous.Ok.deposit_id);
    expect(phaseName((await (bridge.actor as any).get_deposit(ambiguous.Ok.deposit_id))[0].state)).toBe(
      "Cancelled",
    );
    expect(await (ledger.actor as any).ledger_transactions()).toHaveLength(1);
  });

  it("fails a retryable fee payout without trapping its reserve", async () => {
    const { ledger, evm, bridge } = await setup();
    await (evm.actor as any).set_max_service_fee(200_000n);
    await (evm.actor as any).set_service_fee(200_000n);
    const deposit: any = await (bridge.actor as any).request_deposit({ owner_sequence: 0n, base_recipient: new Uint8Array(20).fill(4), from_subaccount: [], gross_amount: 900_000n, max_service_fee: 200_000n });
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
      const claimProfile = IDL.Record({
        instructions: IDL.Nat64,
        storage_revision_before: IDL.Nat64,
        storage_revision_after: IDL.Nat64,
        outcome: IDL.Text,
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
        profile_due_settlement_claim: IDL.Func(
          [],
          [IDL.Variant({ Ok: claimProfile, Err: error })],
          [],
        ),
        profile_rejected_manual_settlement_claim: IDL.Func(
          [IDL.Vec(IDL.Nat8)],
          [IDL.Variant({ Ok: claimProfile, Err: error })],
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

    const firstSeed: any = await maintenance.seed_storage_test_data(0n, 100);
    expect(firstSeed.Ok).toBe(100);

    const rejectedId = createHash("sha256")
      .update("KINIC_BRIDGE_STORAGE_SEED_V2")
      .update(Buffer.from([0, 0, 0, 0, 0, 0, 0, 1]))
      .digest();
    const stableBeforeRejection = await pic!.getStableMemory(bridge.canisterId);
    const rejected: any = await maintenance.profile_rejected_manual_settlement_claim(rejectedId);
    const stableAfterRejection = await pic!.getStableMemory(bridge.canisterId);
    expect(rejected.Ok.outcome).toBe("claimed");
    expect(rejected.Ok.storage_revision_after).toBe(rejected.Ok.storage_revision_before + 1n);
    expect(Buffer.from(stableAfterRejection).equals(Buffer.from(stableBeforeRejection))).toBe(false);

    async function measureDueClaims(): Promise<bigint[]> {
      const samples: bigint[] = [];
      for (let sample = 0; sample < 5; sample += 1) {
        const profile: any = await maintenance.profile_due_settlement_claim();
        expect(profile.Ok.outcome).toBe("claimed");
        expect(profile.Ok.storage_revision_after).toBe(profile.Ok.storage_revision_before + 1n);
        samples.push(profile.Ok.instructions);
      }
      return samples;
    }
    const median = (values: bigint[]) => [...values].sort((left, right) => left < right ? -1 : left > right ? 1 : 0)[2];
    const hundredJobInstructions = await measureDueClaims();

    for (let start = 100; start < 10_000; start += 100) {
      const seeded: any = await maintenance.seed_storage_test_data(BigInt(start), 100);
      expect(seeded.Ok).toBe(100);
    }
    const tenThousandJobInstructions = await measureDueClaims();
    expect(median(tenThousandJobInstructions)).toBeLessThanOrEqual(
      median(hundredJobInstructions) * 2n,
    );
    const before: any = await (bridge.actor as any).get_bridge_status();
    expect(before.schema_version).toBe(35);
    expect(before.counts.withdrawals).toBe(10_000n);
    expect(before.counts.retained_audit_events).toBe(10_000n);
    expect(
      before.settlement_scheduler.scheduled
        + before.settlement_scheduler.leased
        + before.settlement_scheduler.stopped,
    ).toBe(10_000n);
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
    expect(after.schema_version).toBe(35);
    expect(after.counts).toEqual(before.counts);
    expect(after.settlement_scheduler.scheduled).toBe(before.settlement_scheduler.scheduled);
    expect(after.settlement_scheduler.leased).toBe(before.settlement_scheduler.leased);
    expect(after.settlement_scheduler.stopped).toBe(before.settlement_scheduler.stopped);
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
