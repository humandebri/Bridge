import { execFileSync, spawn } from "node:child_process"
import { createServer } from "node:http"
import { mkdir, readFile, writeFile } from "node:fs/promises"
import { gunzipSync } from "node:zlib"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { IDL } from "@dfinity/candid"
import { Ed25519KeyIdentity } from "@icp-sdk/core/identity"
import { Principal } from "@dfinity/principal"
import { PocketIc, PocketIcServer, SubnetStateType } from "@dfinity/pic"
import { createPublicClient, decodeEventLog, hexToBytes, http, numberToHex, recoverTransactionAddress, sha256 } from "viem"
import { publicKeyToAddress } from "viem/accounts"
import { idlFactory as bridgeIdl, init as bridgeInitFactory } from "./generated/bridge.idl.mjs"
import { idlFactory as mockIdl, init as mockInitFactory } from "./generated/mock-external.idl.mjs"

const here = path.dirname(fileURLToPath(import.meta.url))
const uiRoot = path.resolve(here, "..")
const root = path.resolve(uiRoot, "..")
const testTarget = path.join(root, "target", "test-deployment")
const runtimeDir = path.join(uiRoot, ".e2e-runtime")
const rpcUrl = "http://127.0.0.1:8545"
const controlPort = 43119
const uiPort = 4174
const deployer = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
const runtimeAdministrator = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
const baseAdmin = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"
const testIdentity = Ed25519KeyIdentity.generate(new Uint8Array(32).fill(7))
const testOwner = testIdentity.getPrincipal()
const minter = Principal.selfAuthenticating(new Uint8Array(32).fill(9))
const bridgeAbi = JSON.parse(await readFile(path.join(root, "contracts/abi/Bridge.json"), "utf8"))
const bsnsAbi = JSON.parse(await readFile(path.join(root, "contracts/abi/BSNS.json"), "utf8"))
const resources = {}
const bridgeInitType = bridgeInitFactory({ IDL })[0]
const mockInitType = mockInitFactory({ IDL })[0]

export default async function globalSetup() {
  try {
    return await setup()
  } catch (error) {
    await cleanup()
    throw error
  }
}

async function setup() {
  await mkdir(runtimeDir, { recursive: true })
  buildWasm()
  execFileSync("forge", ["build", "--root", path.join(root, "contracts")], { stdio: "inherit" })

  const anvil = spawn("anvil", [
    "--chain-id", "31337", "--base-fee", "1", "--silent",
    "--cache-path", path.join(runtimeDir, "anvil-cache"),
  ], { stdio: ["ignore", "ignore", "inherit"] })
  resources.anvil = anvil
  await waitForRpc()
  const publicClient = createPublicClient({ transport: http(rpcUrl) })

  const picServer = await PocketIcServer.start()
  resources.picServer = picServer
  const pic = await PocketIc.create(picServer.getUrl(), {
    nns: { state: { type: SubnetStateType.New } },
    fiduciary: { state: { type: SubnetStateType.New } },
  })
  await pic.resetTime()
  resources.pic = pic
  const subnet = await pic.getFiduciarySubnet()
  if (!subnet) throw new Error("PocketIC fiduciary subnet is unavailable")

  const ledgerId = await pic.createCanister({ targetSubnetId: subnet.id })
  const ledgerWasm = gunzipSync(await readFile(path.join(uiRoot, ".e2e-cache/ic-icrc1-ledger.wasm.gz")))
  await pic.installCode({
    canisterId: ledgerId,
    wasm: ledgerWasm,
    arg: IDL.encode([ledgerInitType()], [{
      Init: {
        token_symbol: "TICRC1",
        token_name: "TEST ICRC1",
        decimals: [8],
        minting_account: account(minter),
        transfer_fee: 10_000n,
        metadata: [],
        initial_balances: [[account(testOwner), 100_000_000_000n]],
        archive_options: {
          num_blocks_to_archive: 1_000n,
          trigger_threshold: 2_000n,
          controller_id: testOwner,
        },
        feature_flags: [{ icrc2: true }],
      },
    }]),
    targetSubnetId: subnet.id,
  })

  const indexId = await pic.createCanister({ targetSubnetId: subnet.id })
  const indexWasm = gunzipSync(await readFile(path.join(uiRoot, ".e2e-cache/ic-icrc1-index-ng.wasm.gz")))
  const indexInit = IDL.Record({
    ledger_id: IDL.Principal,
    retrieve_blocks_from_ledger_interval_seconds: IDL.Opt(IDL.Nat64),
    min_retrieve_blocks_from_ledger_interval_seconds: IDL.Opt(IDL.Nat64),
    max_retrieve_blocks_from_ledger_interval_seconds: IDL.Opt(IDL.Nat64),
  })
  const indexUpgrade = IDL.Record({
    ledger_id: IDL.Opt(IDL.Principal),
    retrieve_blocks_from_ledger_interval_seconds: IDL.Opt(IDL.Nat64),
    min_retrieve_blocks_from_ledger_interval_seconds: IDL.Opt(IDL.Nat64),
    max_retrieve_blocks_from_ledger_interval_seconds: IDL.Opt(IDL.Nat64),
  })
  await pic.installCode({
    canisterId: indexId,
    wasm: indexWasm,
    arg: IDL.encode([IDL.Opt(IDL.Variant({ Init: indexInit, Upgrade: indexUpgrade }))], [[{ Init: {
      ledger_id: ledgerId,
      retrieve_blocks_from_ledger_interval_seconds: [],
      min_retrieve_blocks_from_ledger_interval_seconds: [1n],
      max_retrieve_blocks_from_ledger_interval_seconds: [1n],
    } }]]),
    targetSubnetId: subnet.id,
  })

  const mock = await pic.setupCanister({
    idlFactory: mockIdl,
    wasm: await readFile(path.join(root, "target/wasm32-unknown-unknown/release/mock_external.wasm")),
    arg: IDL.encode([mockInitType], [{ ledger_id: ledgerId }]),
    cycles: 50_000_000_000_000n,
    targetSubnetId: subnet.id,
  })
  await mock.actor.set_configured_chain_id(31_337n)
  await mock.actor.set_service_fee(1_000_000n)
  await mock.actor.set_max_service_fee(100_000_000n)
  await mock.actor.set_per_deposit_limit(1_000_000_000_000n)
  await mock.actor.set_mint_window(0n, 10_000_000_000_000n, 0n, 3_600n, 1n)
  const bridgeId = await pic.createCanister({ targetSubnetId: subnet.id })
  const mockWasm = await readFile(path.join(root, "target/wasm32-unknown-unknown/release/mock_external.wasm"))
  await pic.installCode({
    canisterId: bridgeId,
    wasm: mockWasm,
    arg: IDL.encode([mockInitType], [{ ledger_id: ledgerId }]),
    targetSubnetId: subnet.id,
  })
  const signerProbe = pic.createActor(mockIdl, bridgeId)
  signerProbe.setIdentity(testIdentity)
  const probe = await signerProbe.probe_chain_key("key_1")
  if (!("Ok" in probe)) throw new Error(`PocketIC chain-key probe failed: ${probe.Err}`)
  let signer = publicKeyToAddress(bytesHex(probe.Ok.public_key))
  await rpc("anvil_setBalance", [signer, "0x8ac7230489e80000"])
  if (BigInt(await rpc("eth_getBalance", [signer, "latest"])) === 0n) throw new Error("Failed to fund the PocketIC Bridge signer")

  const timelockAddress = deployTimelock()
  resources.timelockAddress = timelockAddress
  const bridgeAddress = deployBridge(signer, timelockAddress)
  const bsnsAddress = execFileSync("cast", ["call", bridgeAddress, "bsns()(address)", "--rpc-url", rpcUrl], { encoding: "utf8" }).trim()
  const deploymentBlock = await publicClient.getBlockNumber()
  const bridgeCode = await publicClient.getCode({ address: bridgeAddress })
  const bsnsCode = await publicClient.getCode({ address: bsnsAddress })
  if (!bridgeCode || !bsnsCode) throw new Error("Anvil contract deployment returned empty code")
  await mock.actor.set_bridge_runtime_code(hexToBytes(bridgeCode))

  await pic.reinstallCode({
    canisterId: bridgeId,
    wasm: await readFile(path.join(testTarget, "wasm32-unknown-unknown/release/bridge_canister.wasm")),
    arg: IDL.encode([bridgeInitType], [{
      ledger_canister_id: ledgerId,
      index_canister_id: indexId,
      evm_rpc_canister_id: mock.canisterId,
      custom_evm_rpc_urls: [],
      base_chain_id: 31_337n,
      bridge_contract: hexToBytes(bridgeAddress),
      ecdsa_key_name: "key_1",
      ecdsa_derivation_path: [],
      deposit_rate_limit_window_seconds: 60n,
      deposit_rate_limit_global: 30,
      deposit_rate_limit_per_principal: 3,
      settlement_rate_limit_window_seconds: 600n,
      settlement_rate_limit_global: 60,
      settlement_rate_limit_per_principal: 6,
      settlement_rate_limit_per_record: 3,
      transaction_gas_limit: 500_000n,
      max_fee_per_gas: 10n,
      max_priority_fee_per_gas: 1n,
      eth_floor_wei: 1n,
      cycles_floor: 1n,
      settlement_cycle_ceiling: 1n,
      governance_principal: testOwner,
      pause_principals: [testOwner],
      finance_administrator: testOwner,
      fee_recipient: { owner: testOwner, subaccount: [] },
    }]),
    cycles: 500_000_000_000_000n,
    targetSubnetId: subnet.id,
  })
  const bridgeActor = pic.createActor(bridgeIdl, bridgeId)
  const bridge = { actor: bridgeActor, canisterId: bridgeId }
  bridge.actor.setIdentity(testIdentity)
  const resumeResult = await bridge.actor.resume_new_deposits()
  if (!("Ok" in resumeResult)) throw new Error(`Failed to activate test Bridge canister: ${JSON.stringify(resumeResult.Err)}`)
  mock.actor.setIdentity(testIdentity)
  const configuredSigner = await mock.actor.set_bridge_signer_for_canister(bridgeId, "key_1")
  if (!("Ok" in configuredSigner)) throw new Error(`Failed to configure the confirmed bridge signer: ${configuredSigner.Err}`)
  const confirmedSigner = bytesHex(await mock.actor.bridge_signer())
  if (confirmedSigner.toLowerCase() !== signer.toLowerCase()) {
    sendAsTimelock(bridgeAddress, "rotateBridgeSigner(address)", confirmedSigner)
    signer = confirmedSigner
  }
  await rpc("anvil_setBalance", [signer, "0x8ac7230489e80000"])
  await rpc("anvil_mine", ["0x40"])
  const initialSafe = await publicClient.getBlock({ blockTag: "safe" })
  if (initialSafe.number === null) throw new Error("Anvil initial safe head is unavailable")
  const initialSafeResult = await mock.actor.set_safe_block(initialSafe.number, hexToBytes(initialSafe.hash))
  if ("Err" in initialSafeResult) throw new Error(initialSafeResult.Err)
  const ledger = pic.createActor(ledgerIdl, ledgerId)
  ledger.setIdentity(testIdentity)
  const index = pic.createActor(indexIdl, indexId)
  const publicConfig = await bridge.actor.get_public_config()
  await pic.advanceTime(2_000)
  await pic.tick(30)

  const gatewayPort = await pic.client.startHttpGateway()
  resources.gatewayClient = pic.client
  startProgressLoop(pic)
  await writeProfile({
    gatewayPort,
    ledgerId: ledgerId.toText(),
    indexId: indexId.toText(),
    bridgeId: bridge.canisterId.toText(),
    evmRpcCanisterId: publicConfig.evm_rpc_canister_id.toText(),
    rpcProviderUrlsSha256: bytesHex(publicConfig.rpc_provider_urls_sha256),
    bridgeAddress,
    bsnsAddress,
    expected_bridge_signer: signer,
    deploymentBlock,
    bridgeHash: sha256(bridgeCode),
    bsnsHash: sha256(bsnsCode),
  })

  let relayedBroadcasts = 0
  let notifyCalls = 0
  let failNextNotification = false
  let failNextDepositResponse = false
  let connectedAccount = testOwner.toText()
  const knownDeposits = []
  const depositSequences = []
  const knownWithdrawals = []
  const syncObservedHeads = async () => {
    const [safe, finalized] = await Promise.all([
      publicClient.getBlock({ blockTag: "safe" }),
      publicClient.getBlock({ blockTag: "finalized" }),
    ])
    if (safe.number === null || safe.hash === null) throw new Error("Anvil safe head is unavailable")
    if (finalized.number === null || finalized.hash === null) throw new Error("Anvil finalized head is unavailable")
    const [safeResult, finalizedResult] = await Promise.all([
      mock.actor.set_safe_block(safe.number, hexToBytes(safe.hash)),
      mock.actor.set_finalized_block(finalized.number, hexToBytes(finalized.hash)),
    ])
    if ("Err" in safeResult) throw new Error(safeResult.Err)
    if ("Err" in finalizedResult) throw new Error(finalizedResult.Err)
  }
  await syncObservedHeads()
  const relayPendingBroadcasts = async () => {
    const broadcasts = await mock.actor.broadcast_transactions()
    for (; relayedBroadcasts < broadcasts.length; relayedBroadcasts += 1) {
      const raw = bytesHex(broadcasts[relayedBroadcasts])
      const rawSigner = await recoverTransactionAddress({ serializedTransaction: raw })
      if (rawSigner.toLowerCase() !== signer.toLowerCase()) {
        sendAsTimelock(bridgeAddress, "rotateBridgeSigner(address)", rawSigner)
        await rpc("anvil_setBalance", [rawSigner, "0x8ac7230489e80000"])
        signer = rawSigner
      }
      try { await rpc("eth_sendRawTransaction", [raw]) } catch (error) {
        if (!String(error).includes("already known")) throw error
      }
    }
    await rpc("evm_mine", [])
    // Anvil's Safe tag trails the tip. Mine enough descendants so receipts
    // relayed in this round can be confirmed by the canister's Safe checks.
    await rpc("anvil_mine", ["0x40"])
  }
  const control = createServer(async (request, response) => {
    response.setHeader("access-control-allow-origin", `http://127.0.0.1:${uiPort}`)
    response.setHeader("access-control-allow-headers", "content-type")
    if (request.method === "OPTIONS") return send(response, 204, null)
    try {
      const body = request.method === "POST" ? await readJson(request) : undefined
      if (request.url === "/ic/account") return send(response, 200, { owner: connectedAccount })
      if (request.url === "/ic/disconnect") return send(response, 200, null)
      if (request.url === "/ic/approve") {
        const now = await pic.getTime()
        const result = await ledger.icrc2_approve({
          from_subaccount: [],
          spender: account(bridge.canisterId),
          amount: BigInt(body.amount),
          expected_allowance: [BigInt(body.currentAllowance)],
          expires_at: [BigInt(now + 30 * 60 * 1_000) * 1_000_000n],
          fee: [BigInt(body.ledgerFee)],
          memo: [],
          created_at_time: [],
        })
        if ("Err" in result) throw new Error(`ledger approve failed: ${json(result.Err)}`)
        return send(response, 200, result.Ok.toString())
      }
      if (request.url === "/ic/deposit") {
        if (connectedAccount !== testOwner.toText()) throw new Error("test IC account changed")
        depositSequences.push(String(body.ownerSequence))
        const result = await bridge.actor.request_deposit({
          owner_sequence: BigInt(body.ownerSequence),
          base_recipient: hexToBytes(body.baseRecipient),
          from_subaccount: [],
          gross_amount: BigInt(body.grossAmount),
          max_service_fee: BigInt(body.maxServiceFee),
        })
        if ("Err" in result) throw new Error(`deposit rejected: ${json(result.Err)}`)
        if (!knownDeposits.some((id) => bytesHex(id) === bytesHex(result.Ok.deposit_id))) knownDeposits.push(result.Ok.deposit_id)
        if (failNextDepositResponse) {
          failNextDepositResponse = false
          return send(response, 503, { error: "Injected response loss after deposit acceptance" })
        }
        return send(response, 200, { deposit_id: bytesHex(result.Ok.deposit_id), owner_sequence: result.Ok.owner_sequence.toString(), state: result.Ok.state, settlement: result.Ok.settlement[0] ? settlementJson(result.Ok.settlement[0]) : undefined })
      }
      if (request.url === "/ic/notify") {
        notifyCalls += 1
        if (failNextNotification) {
          failNextNotification = false
          return send(response, 503, { error: "Injected notification failure" })
        }
        if (connectedAccount !== testOwner.toText()) throw new Error("test IC account changed")
        const transactionHash = body.transactionHash
        const receipt = await publicClient.getTransactionReceipt({ hash: transactionHash })
        const created = receipt.logs.map((log) => {
          try { return decodeEventLog({ abi: bridgeAbi, eventName: "WithdrawalCommitted", data: log.data, topics: log.topics }) } catch { return undefined }
        }).find(Boolean)
        if (!created) throw new Error("WithdrawalCommitted log is missing from the Anvil receipt")
        await mock.actor.set_withdrawal([{ id: hexToBytes(numberToHex(created.args.withdrawalId, { size: 32 })), owner: hexToBytes(created.args.owner), subaccount: hexToBytes(created.args.subaccount), amount: created.args.amount, max_service_fee: created.args.maxServiceFee, charged_service_fee: created.args.chargedServiceFee, amount_out: created.args.amountOut }])
        const observed = await mock.actor.set_observed_transaction(hexToBytes(transactionHash), hexToBytes(bridgeAddress), hexToBytes(created.args.requester), Number(receipt.blockNumber))
        if ("Err" in observed) throw new Error(observed.Err)
        await syncObservedHeads()
        const result = await bridge.actor.notify_withdrawal({ transaction_hash: hexToBytes(transactionHash) })
        if ("Err" in result) throw new Error(`withdrawal notification rejected: ${json(result.Err)}`)
        const withdrawalId = "Ingested" in result.Ok ? result.Ok.Ingested.withdrawal_id : result.Ok.Duplicate.withdrawal_id
        if (!knownWithdrawals.some((id) => bytesHex(id) === bytesHex(withdrawalId))) knownWithdrawals.push(withdrawalId)
        if ("Ingested" in result.Ok) return send(response, 200, { Ingested: { finalized_head_block_number: result.Ok.Ingested.finalized_head_block_number.toString(), withdrawal_id: bytesHex(result.Ok.Ingested.withdrawal_id), settlement: result.Ok.Ingested.settlement[0] ? settlementJson(result.Ok.Ingested.settlement[0]) : undefined } })
        return send(response, 200, { Duplicate: { withdrawal_id: bytesHex(result.Ok.Duplicate.withdrawal_id), settlement: result.Ok.Duplicate.settlement[0] ? settlementJson(result.Ok.Duplicate.settlement[0]) : undefined } })
      }
      if (request.url === "/ic/continue-deposit") {
        const result = await bridge.actor.continue_deposit(hexToBytes(body.id))
        if ("Err" in result) throw new Error(`deposit continuation rejected: ${json(result.Err)}`)
        return send(response, 200, settlementJson(result.Ok))
      }
      if (request.url === "/ic/continue-withdrawal") {
        const result = await bridge.actor.continue_withdrawal(hexToBytes(body.id))
        if ("Err" in result) throw new Error(`withdrawal continuation rejected: ${json(result.Err)}`)
        return send(response, 200, settlementJson(result.Ok))
      }
      if (request.url === "/test/fail-next-notification") {
        failNextNotification = true
        return send(response, 200, null)
      }
      if (request.url === "/test/fail-next-deposit-response") {
        failNextDepositResponse = true
        return send(response, 200, null)
      }
      if (request.url === "/test/settle") {
        for (let round = 0; round < 10; round += 1) {
          for (const id of knownDeposits) await bridge.actor.continue_deposit(id)
          for (const id of knownWithdrawals) await bridge.actor.continue_withdrawal(id)
          await relayPendingBroadcasts()
        }
        return send(response, 200, null)
      }
      if (request.url === "/test/relay") {
        await relayPendingBroadcasts()
        await syncObservedHeads()
        stopProgressLoop()
        try {
          await pic.advanceTime(2_000)
          await pic.tick(30)
        } finally {
          startProgressLoop(pic)
        }
        return send(response, 200, null)
      }
      if (request.url === "/test/advance-confirmation") {
        await relayPendingBroadcasts()
        await syncObservedHeads()
        stopProgressLoop()
        try {
          await pic.advanceTime(Number(body.minutes ?? 20) * 60_000)
          await pic.tick(100)
          await relayPendingBroadcasts()
          await syncObservedHeads()
          // Let the prior cached observation expire so the next UI readiness
          // refresh must consume the newly synchronized finalized head.
          await pic.advanceTime(31_000)
          await pic.tick(5)
        } finally {
          startProgressLoop(pic)
        }
        return send(response, 200, { time: await pic.getTime() })
      }
      if (request.url === "/test/account") {
        connectedAccount = String(body.owner)
        return send(response, 200, null)
      }
      if (request.url === "/test/state") {
        const balance = await publicClient.readContract({ address: bsnsAddress, abi: bsnsAbi, functionName: "balanceOf", args: [deployer] })
        const [allowance, ledgerBalance, indexBalance, indexLedgerId, indexStatus, nextDepositSequence] = await Promise.all([
          publicClient.readContract({ address: bsnsAddress, abi: bsnsAbi, functionName: "allowance", args: [deployer, bridgeAddress] }),
          ledger.icrc1_balance_of(account(testOwner)),
          index.icrc1_balance_of(account(testOwner)),
          index.ledger_id(),
          index.status(),
          bridge.actor.get_next_deposit_sequence(testOwner),
        ])
        return send(response, 200, {
          broadcasts: relayedBroadcasts,
          notifyCalls,
          knownDepositCount: knownDeposits.length,
          depositSequences,
          nextDepositSequence: nextDepositSequence.toString(),
          bsnsBalance: balance.toString(),
          bsnsAllowance: allowance.toString(),
          ledgerBalance: ledgerBalance.toString(),
          ledgerId: ledgerId.toText(),
          indexBalance: indexBalance.toString(),
          indexLedgerId: indexLedgerId.toText(),
          indexBlocksSynced: indexStatus.num_blocks_synced.toString(),
        })
      }
      return send(response, 404, { error: "not found" })
    } catch (error) {
      console.error(`[real-e2e control] ${request.url}:`, error)
      return send(response, 500, { error: error instanceof Error ? error.message : String(error) })
    }
  })
  await new Promise((resolve, reject) => control.once("error", reject).listen(controlPort, "127.0.0.1", resolve))
  resources.control = control

  const vite = spawn("pnpm", ["exec", "vite", "--config", "vite.real.config.ts", "--host", "127.0.0.1", "--port", String(uiPort)], { cwd: uiRoot, stdio: "inherit" })
  resources.vite = vite
  await waitForUrl(`http://127.0.0.1:${uiPort}`)

  return cleanup
}

async function cleanup() {
  resources.vite?.kill("SIGTERM")
  if (resources.control?.listening) await new Promise((resolve) => resources.control.close(resolve))
  stopProgressLoop()
  await resources.gatewayClient?.stopHttpGateway().catch(() => undefined)
  await resources.pic?.tearDown().catch(() => undefined)
  await resources.picServer?.stop().catch(() => undefined)
  resources.anvil?.kill("SIGTERM")
}

function startProgressLoop(pic) {
  stopProgressLoop()
  let ticking = false
  resources.progressTimer = setInterval(() => {
    if (ticking) return
    ticking = true
    void pic.tick().finally(() => { ticking = false })
  }, 20)
}

function stopProgressLoop() {
  if (resources.progressTimer !== undefined) clearInterval(resources.progressTimer)
  resources.progressTimer = undefined
}

function buildWasm() {
  execFileSync("cargo", ["build", "--target", "wasm32-unknown-unknown", "--release", "-p", "bridge-canister", "--features", "test-deployment"], { cwd: root, stdio: "inherit", env: { ...process.env, CARGO_TARGET_DIR: testTarget } })
  execFileSync("cargo", ["build", "--target", "wasm32-unknown-unknown", "--release", "-p", "mock-external"], { cwd: root, stdio: "inherit" })
}

function deployTimelock() {
  const output = execFileSync("forge", [
    "create", "--root", path.join(root, "contracts"), "--rpc-url", rpcUrl,
    "--from", deployer, "--unlocked", "--broadcast",
    "src/BridgeTimelockController.sol:BridgeTimelockController", "--constructor-args",
    "259200", `[${baseAdmin}]`, `[${runtimeAdministrator}]`, `[${baseAdmin}]`,
  ], { encoding: "utf8" })
  const match = output.match(/Deployed to:\s*(0x[0-9a-fA-F]{40})/)
  if (!match) throw new Error(`Unable to parse Timelock deployment:\n${output}`)
  return match[1]
}

function deployBridge(signer, timelockAddress) {
  const timelockCodeHash = execFileSync(
    "cast", ["codehash", timelockAddress, "--rpc-url", rpcUrl], { encoding: "utf8" },
  ).trim()
  const output = execFileSync("forge", [
    "create", "--root", path.join(root, "contracts"), "--rpc-url", rpcUrl,
    "--from", deployer, "--unlocked", "--broadcast", "src/Bridge.sol:Bridge", "--constructor-args",
    "Bridged KINIC", "KINIC", "8", signer, runtimeAdministrator, timelockAddress, timelockCodeHash,
    "1000000000000", "10000000000000", "3600", "100000000", "1000000",
  ], { encoding: "utf8" })
  const match = output.match(/Deployed to:\s*(0x[0-9a-fA-F]{40})/)
  if (!match) throw new Error(`Unable to parse Bridge deployment:\n${output}`)
  sendAsTimelock(match[1], "unpauseDepositMints()")
  sendAsTimelock(match[1], "unpauseWithdrawals()")
  return match[1]
}

function sendAsTimelock(target, signature, ...args) {
  const timelockAddress = resources.timelockAddress
  if (!timelockAddress) throw new Error("Timelock has not been deployed")
  execFileSync("cast", ["rpc", "anvil_setBalance", timelockAddress, "0x56BC75E2D63100000", "--rpc-url", rpcUrl])
  execFileSync("cast", ["rpc", "anvil_impersonateAccount", timelockAddress, "--rpc-url", rpcUrl])
  try {
    execFileSync("cast", ["send", target, signature, ...args, "--from", timelockAddress, "--unlocked", "--rpc-url", rpcUrl], { stdio: "inherit" })
  } finally {
    execFileSync("cast", ["rpc", "anvil_stopImpersonatingAccount", timelockAddress, "--rpc-url", rpcUrl])
  }
}

async function writeProfile(values) {
  const source = `
export interface DeploymentProfile {
  environment: string; label: string; testOnly: boolean;
  icHost: string; baseRpcUrl: string; chainId: number; bridgeCanisterId: string | null; ledgerCanisterId: string | null; indexCanisterId: string | null;
  evmRpcCanisterId: string | null; rpcProviderUrlsSha256: \`0x\${string}\` | null;
  icToken: { name: string; symbol: string; decimals: number }; baseToken: { symbol: string; decimals: number };
  bridgeAddress: \`0x\${string}\` | null; bsnsAddress: \`0x\${string}\` | null; expected_bridge_signer: \`0x\${string}\` | null; deploymentBlock: bigint | null;
  bridgeRuntimeHash: \`0x\${string}\` | null; bsnsRuntimeHash: \`0x\${string}\` | null;
}
export const deploymentProfile: DeploymentProfile = ${serialize({
    environment: "local-real-e2e", label: "Local Anvil + PocketIC", testOnly: true,
    icHost: `http://127.0.0.1:${values.gatewayPort}`,
    baseRpcUrl: rpcUrl, chainId: 31337, bridgeCanisterId: values.bridgeId, ledgerCanisterId: values.ledgerId, indexCanisterId: values.indexId,
    evmRpcCanisterId: values.evmRpcCanisterId, rpcProviderUrlsSha256: values.rpcProviderUrlsSha256,
    icToken: { name: "TEST ICRC1", symbol: "TICRC1", decimals: 8 }, baseToken: { symbol: "KINIC", decimals: 8 },
    bridgeAddress: values.bridgeAddress, bsnsAddress: values.bsnsAddress, expected_bridge_signer: values.expected_bridge_signer, deploymentBlock: values.deploymentBlock,
    bridgeRuntimeHash: values.bridgeHash, bsnsRuntimeHash: values.bsnsHash,
  })}
export function profileCompleteness(profile: DeploymentProfile): string[] {
  const blockers: string[] = []
  if (!profile.bridgeCanisterId || !profile.ledgerCanisterId || !profile.indexCanisterId || !profile.evmRpcCanisterId || !profile.rpcProviderUrlsSha256 || !profile.bridgeAddress || !profile.bsnsAddress || !profile.expected_bridge_signer || profile.deploymentBlock === null || !profile.bridgeRuntimeHash || !profile.bsnsRuntimeHash) blockers.push("Deployment profile is incomplete")
  return blockers
}
`
  await writeFile(path.join(runtimeDir, "profile.ts"), source)
}

function serialize(value) {
  if (typeof value === "bigint") return `${value}n`
  if (Array.isArray(value)) return `[${value.map(serialize).join(",")}]`
  if (value && typeof value === "object") return `{${Object.entries(value).map(([key, item]) => `${JSON.stringify(key)}:${serialize(item)}`).join(",")}}`
  return JSON.stringify(value)
}

function account(owner) { return { owner, subaccount: [] } }
function bytesHex(bytes) { return `0x${Buffer.from(bytes).toString("hex")}` }
function json(value) { return JSON.stringify(value, (_key, item) => typeof item === "bigint" ? item.toString() : item) }
function settlementJson(value) {
  if ("Submitted" in value) return { Submitted: { ...value.Submitted, transaction_hash: Array.from(value.Submitted.transaction_hash) } }
  if ("WaitingForConfirmation" in value) return { WaitingForConfirmation: { ...value.WaitingForConfirmation, transaction_hash: Array.from(value.WaitingForConfirmation.transaction_hash) } }
  return value
}
async function readJson(request) { const chunks = []; for await (const chunk of request) chunks.push(chunk); return JSON.parse(Buffer.concat(chunks).toString("utf8") || "null") }
function send(response, status, value) { response.statusCode = status; response.setHeader("content-type", "application/json"); response.end(value === null ? "null" : JSON.stringify(value)); }
async function rpc(method, params) { const response = await fetch(rpcUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }) }); const value = await response.json(); if (value.error) throw new Error(value.error.message); return value.result }
async function waitForRpc() { for (let attempt = 0; attempt < 100; attempt += 1) { try { if (await rpc("eth_chainId", []) === "0x7a69") return } catch {} await delay(100) } throw new Error("Anvil did not start") }
async function waitForUrl(url) { for (let attempt = 0; attempt < 200; attempt += 1) { try { if ((await fetch(url)).ok) return } catch {} await delay(100) } throw new Error(`${url} did not start`) }
function delay(ms) { return new Promise((resolve) => setTimeout(resolve, ms)) }

function ledgerInitType() {
  const Account = IDL.Record({ owner: IDL.Principal, subaccount: IDL.Opt(IDL.Vec(IDL.Nat8)) })
  const Value = IDL.Variant({ Nat: IDL.Nat, Int: IDL.Int, Text: IDL.Text, Blob: IDL.Vec(IDL.Nat8) })
  return IDL.Variant({ Init: IDL.Record({
    token_symbol: IDL.Text, token_name: IDL.Text, decimals: IDL.Opt(IDL.Nat8), minting_account: Account, transfer_fee: IDL.Nat,
    metadata: IDL.Vec(IDL.Tuple(IDL.Text, Value)), initial_balances: IDL.Vec(IDL.Tuple(Account, IDL.Nat)),
    archive_options: IDL.Record({ num_blocks_to_archive: IDL.Nat64, trigger_threshold: IDL.Nat64, controller_id: IDL.Principal }),
    feature_flags: IDL.Opt(IDL.Record({ icrc2: IDL.Bool })),
  }) })
}

const ledgerIdl = ({ IDL: I }) => {
  const Account = I.Record({ owner: I.Principal, subaccount: I.Opt(I.Vec(I.Nat8)) })
  const ApproveError = I.Variant({
    BadFee: I.Record({ expected_fee: I.Nat }), InsufficientFunds: I.Record({ balance: I.Nat }),
    AllowanceChanged: I.Record({ current_allowance: I.Nat }), Expired: I.Record({ ledger_time: I.Nat64 }),
    TooOld: I.Null, CreatedInFuture: I.Record({ ledger_time: I.Nat64 }), Duplicate: I.Record({ duplicate_of: I.Nat }),
    TemporarilyUnavailable: I.Null, GenericError: I.Record({ error_code: I.Nat, message: I.Text }),
  })
  return I.Service({
    icrc2_approve: I.Func([I.Record({ from_subaccount: I.Opt(I.Vec(I.Nat8)), spender: Account, amount: I.Nat, expected_allowance: I.Opt(I.Nat), expires_at: I.Opt(I.Nat64), fee: I.Opt(I.Nat), memo: I.Opt(I.Vec(I.Nat8)), created_at_time: I.Opt(I.Nat64) })], [I.Variant({ Ok: I.Nat, Err: ApproveError })], []),
    icrc1_balance_of: I.Func([Account], [I.Nat], ["query"]),
  })
}

const indexIdl = ({ IDL: I }) => {
  const Account = I.Record({ owner: I.Principal, subaccount: I.Opt(I.Vec(I.Nat8)) })
  return I.Service({
    ledger_id: I.Func([], [I.Principal], ["query"]),
    status: I.Func([], [I.Record({ num_blocks_synced: I.Nat })], ["query"]),
    icrc1_balance_of: I.Func([Account], [I.Nat], ["query"]),
  })
}
