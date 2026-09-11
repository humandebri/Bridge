import { readFileSync } from "node:fs";
import { spawn, type ChildProcess } from "node:child_process";
import { createServer } from "node:net";
import { createHash } from "node:crypto";
import { resolve } from "node:path";
import { IDL } from "@icp-sdk/core/candid";
import { Principal } from "@icp-sdk/core/principal";
import { PocketIc } from "@dfinity/pic";
import { idlFactory as bridgeIdl, init as bridgeInitFactory } from "./generated/bridge.idl";
import { idlFactory as mockIdl, init as mockInitFactory } from "./generated/mock-external.idl";

const root = resolve(__dirname, "..");
const launcherId = Principal.fromText("xfug4-5qaaa-aaaak-afowa-cai");
const threshold = 2_000_000_000_000n;
const day = 24 * 60 * 60 * 1000;
const controllerPrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(6));
const runtimePrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(7));
const confirmationRelayerPrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(8));
const feeRecipientPrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(55));
const bridgeWasm = resolve(root, "target/test-deployment/staging/bridge_canister.wasm");

// Bootstrap stays unsealed so unrelated settlement timers cannot affect these checks.
const init = { ledger_canister_id: launcherId, index_canister_id: launcherId, evm_rpc_canister_id: launcherId, custom_evm_rpc_urls: [], base_chain_id: 8453n, bridge_contract: new Uint8Array(20).fill(1), expected_bridge_runtime_sha256: new Uint8Array(createHash("sha256").update(new Uint8Array([0x60, 0x00])).digest()), timelock_contract: new Uint8Array(20).fill(2), expected_timelock_minimum_delay_seconds: 300n, expected_bsns_runtime_sha256: new Uint8Array(createHash("sha256").update(new Uint8Array([0x60, 0x02])).digest()), expected_bsns_decimals: 8, expected_minimum_service_fee: 1n, deployment_instance_id: new Uint8Array(32).fill(3), minimum_withdrawal_id: new Uint8Array([...new Uint8Array(31), 1]), ecdsa_key_name: "key_1", ecdsa_derivation_path: [], governance_ecdsa_derivation_path: [new TextEncoder().encode("governance-operator")], deposit_rate_limit_window_seconds: 60n, deposit_rate_limit_global: 30, deposit_rate_limit_per_principal: 3, notification_rate_limit_window_seconds: 600n, notification_rate_limit_global: 60, notification_ingestion_rate_limit_global: 30, settlement_rate_limit_window_seconds: 3_600n, settlement_rate_limit_global: 60, settlement_rate_limit_per_principal: 30, settlement_rate_limit_per_record: 3, settlement_retry_interval_seconds: 60n, governance_evm_fee: { gas_limit_ceiling: 500_000n, max_fee_per_gas_ceiling: 200_000_000_000n, max_priority_fee_per_gas_ceiling: 10_000_000_000n, l1_fee_per_transaction_ceiling_wei: 10_000_000_000_000_000n, quote_validity_seconds: 90n, gas_limit_multiplier_bps: 13_000, base_fee_multiplier_bps: 60_000, l1_fee_multiplier_bps: 15_000 }, governance_replacement: { max_replacements: 3, fee_bump_bps: 1_250 }, cycles_floor: 1n, settlement_cycle_ceiling: 1n, governance_principal: runtimePrincipal, pause_principal: Principal.selfAuthenticating(new Uint8Array(32).fill(34)), confirmation_relayer_principal: confirmationRelayerPrincipal, fee_recipient: { owner: feeRecipientPrincipal, subaccount: [] } };

describe("launcher cycles top-up", () => {
  let server: ChildProcess | undefined;
  let serverUrl: string;
  let pic: PocketIc;
  let launcher: any;
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
    server = spawn(resolve(root, "node_modules/@dfinity/pic/pocket-ic"), ["--port", String(port), "--hard-ttl", "600"], { stdio: "inherit" });
    for (let attempt = 0; attempt < 100; attempt += 1) {
      if (server.exitCode !== null) throw new Error(`PocketIC exited: ${server.exitCode}`);
      try { await fetch(serverUrl); return; }
      catch { await new Promise((ready) => setTimeout(ready, 100)); }
    }
    throw new Error("PocketIC server did not become ready");
  });
  afterAll(async () => { server?.kill(); });
  beforeEach(async () => {
    pic = await PocketIc.create(serverUrl);
    launcher = await pic.setupCanister({
      idlFactory: mockIdl,
      wasm: readFileSync(resolve(root, "target/wasm32-unknown-unknown/release/mock_external.wasm")),
      arg: IDL.encode(mockInitFactory({ IDL }), [{ ledger_id: launcherId }]),
      targetCanisterId: launcherId,
      cycles: 50_000_000_000_000n,
    });
  });
  afterEach(async () => { await pic?.tearDown(); });

  async function install(cycles = 1_500_000_000_000n) {
    const bridge = await pic.setupCanister({
      idlFactory: bridgeIdl, wasm: readFileSync(bridgeWasm),
      arg: IDL.encode(bridgeInitFactory({ IDL }), [init]),
      cycles, sender: controllerPrincipal, freezingThreshold: 0n,
    });
    bridge.actor.setPrincipal(controllerPrincipal);
    return bridge;
  }
  async function count() { return (await launcher.actor.get_cycles_top_up_callers()).length; }

  it("cycles_top_up_checks_after_install_and_upgrade", async () => {
    await launcher.actor.set_cycles_top_up_mode({ Reject: null });
    const bridge = await install();
    await pic.tick(15);
    expect(await count()).toBe(1);
    const before = BigInt(await pic.getCyclesBalance(bridge.canisterId));
    await launcher.actor.set_cycles_top_up_mode({ Succeed: null });
    await pic.upgradeCanister({
      canisterId: bridge.canisterId, wasm: readFileSync(bridgeWasm),
      arg: IDL.encode([], []), sender: controllerPrincipal,
    });
    await pic.tick(15);
    expect(await count()).toBe(2);
    expect(BigInt(await pic.getCyclesBalance(bridge.canisterId))).toBeGreaterThan(before + threshold);
    const callers = await launcher.actor.get_cycles_top_up_callers();
    expect(callers.map((id: Principal) => id.toText())).toEqual([bridge.canisterId.toText(), bridge.canisterId.toText()]);
    await pic.advanceTime(day);
    await pic.tick(15);
    expect(await count()).toBe(2);
  });

  it("cycles_top_up_checks_every_24_hours_without_early_retry", async () => {
    await launcher.actor.set_cycles_top_up_mode({ Reject: null });
    const bridge = await install();
    await pic.tick(15);
    expect(await count()).toBe(1);
    await pic.advanceTime(day - 60_000);
    await pic.tick(15);
    expect(await count()).toBe(1);
    await launcher.actor.set_cycles_top_up_mode({ Succeed: null });
    const before = BigInt(await pic.getCyclesBalance(bridge.canisterId));
    await pic.advanceTime(60_000);
    await pic.tick(15);
    expect(await count()).toBe(2);
    expect(BigInt(await pic.getCyclesBalance(bridge.canisterId))).toBeGreaterThan(before + threshold);
  });

  it("cycles_top_up_manual_check_requires_controller_and_observes_balance", async () => {
    await launcher.actor.set_cycles_top_up_mode({ Reject: null });
    const bridge = await install();
    await pic.tick(15);
    const actor: any = bridge.actor;
    for (const unauthorized of [Principal.anonymous(), runtimePrincipal]) {
      actor.setPrincipal(unauthorized);
      expect(await actor.check_cycles_top_up()).toEqual({ Err: "controller only" });
    }
    expect(await count()).toBe(1);
    actor.setPrincipal(controllerPrincipal);
    expect(await actor.check_cycles_top_up()).toEqual({ Err: "launcher rejected: TooSoon" });
    expect(await count()).toBe(2);
    await launcher.actor.set_cycles_top_up_mode({ Succeed: null });
    const before = BigInt(await pic.getCyclesBalance(bridge.canisterId));
    expect(await actor.check_cycles_top_up()).toEqual({ Ok: null });
    expect(await count()).toBe(3);
    expect(BigInt(await pic.getCyclesBalance(bridge.canisterId))).toBeGreaterThan(before + threshold);
    expect(await actor.check_cycles_top_up()).toEqual({ Ok: null });
    expect(await count()).toBe(3);
    const highBalance = await install(10_000_000_000_000n);
    await pic.tick(15);
    expect(await (highBalance.actor as any).check_cycles_top_up()).toEqual({ Ok: null });
    expect(await count()).toBe(3);
  });
});
