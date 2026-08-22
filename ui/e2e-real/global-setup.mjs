import { execFileSync, spawn } from "node:child_process"
import { createServer } from "node:http"
import { connect } from "node:net"
import { mkdir, readFile, writeFile } from "node:fs/promises"
import { gunzipSync } from "node:zlib"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { IDL } from "@dfinity/candid"
import { Ed25519KeyIdentity } from "@icp-sdk/core/identity"
import { Principal } from "@dfinity/principal"
import { PocketIc, PocketIcServer, SubnetStateType } from "@dfinity/pic"
import { createPublicClient, hexToBytes, http, keccak256, numberToHex, recoverTransactionAddress, sha256 } from "viem"
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
const ACTIVATION_DELAY_SECONDS = 5 * 60
const ACTIVATION_TIME_ADVANCES = 3
const stagingForgeEnv = { ...process.env, FOUNDRY_PROFILE: "staging" }
const deployer = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
const testIdentity = Ed25519KeyIdentity.generate(new Uint8Array(32).fill(7))
const testOwner = testIdentity.getPrincipal()
const pauseIdentity = Ed25519KeyIdentity.generate(new Uint8Array(32).fill(8))
const pausePrincipal = pauseIdentity.getPrincipal()
const minter = Principal.selfAuthenticating(new Uint8Array(32).fill(9))
const feeRecipient = Principal.selfAuthenticating(new Uint8Array(32).fill(10))
const confirmationRelayerPrincipal = Principal.selfAuthenticating(new Uint8Array(32).fill(11))
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
  execFileSync("forge", ["build", "--root", path.join(root, "contracts")], {
    stdio: "inherit",
    env: stagingForgeEnv,
  })

  if (await isTcpPortOpen("127.0.0.1", 8545)) {
    throw new Error("TCP port 8545 is already in use; real E2E never reuses an existing endpoint")
  }
  const anvilGenesisTimestamp = Math.floor(Date.now() / 1_000)
    - ACTIVATION_DELAY_SECONDS * ACTIVATION_TIME_ADVANCES
  const anvil = spawn("anvil", [
    "--chain-id", "31337", "--base-fee", "1", "--silent",
    "--timestamp", String(anvilGenesisTimestamp),
    "--host", "127.0.0.1", "--port", "8545",
    "--cache-path", path.join(runtimeDir, "anvil-cache"),
  ], { stdio: ["ignore", "ignore", "inherit"] })
  resources.anvil = anvil
  await waitForOwnedRpc(anvil)
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
  const bridgeId = await pic.createCanister({ controllers: [testOwner], targetSubnetId: subnet.id })
  const mockWasm = await readFile(path.join(root, "target/wasm32-unknown-unknown/release/mock_external.wasm"))
  await pic.installCode({
    canisterId: bridgeId,
    wasm: mockWasm,
    arg: IDL.encode([mockInitType], [{ ledger_id: ledgerId }]),
    sender: testOwner,
    targetSubnetId: subnet.id,
  })
  const signerProbe = pic.createActor(mockIdl, bridgeId)
  signerProbe.setIdentity(testIdentity)
  let signer = bytesHex(await signerProbe.derive_chain_key_address(bridgeId, "key_1", []))
  const governanceOperator = bytesHex(await signerProbe.derive_chain_key_address(
    bridgeId,
    "key_1",
    [new TextEncoder().encode("governance-operator")],
  ))
  await rpc("anvil_setBalance", [governanceOperator, "0x8ac7230489e80000"])
  if (BigInt(await rpc("eth_getBalance", [governanceOperator, "latest"])) === 0n) throw new Error("Failed to fund the PocketIC governance operator")

  const timelockAddress = deployTimelock(governanceOperator)
  resources.timelockAddress = timelockAddress
  const bridgeAddress = deployBridge(signer, governanceOperator, timelockAddress)
  const bsnsAddress = execFileSync("cast", ["call", bridgeAddress, "bsns()(address)", "--rpc-url", rpcUrl], { encoding: "utf8" }).trim()
  const deploymentBlock = await publicClient.getBlockNumber()
  const bridgeCode = await publicClient.getCode({ address: bridgeAddress })
  const timelockCode = await publicClient.getCode({ address: timelockAddress })
  const bsnsCode = await publicClient.getCode({ address: bsnsAddress })
  if (!bridgeCode || !timelockCode || !bsnsCode) throw new Error("Anvil contract deployment returned empty code")
  await mock.actor.set_bridge_runtime_code(hexToBytes(bridgeCode))
  const operationalConfig = {
    governance_evm_fee: {
      gas_limit_ceiling: 500_000n,
      max_fee_per_gas_ceiling: 200_000_000_000n,
      max_priority_fee_per_gas_ceiling: 10_000_000_000n,
      l1_fee_per_transaction_ceiling_wei: 10_000_000_000_000_000n,
      quote_validity_seconds: 90n,
      gas_limit_multiplier_bps: 13_000,
      base_fee_multiplier_bps: 60_000,
      l1_fee_multiplier_bps: 15_000,
    },
    cycles_floor: 1n,
    settlement_cycle_ceiling: 1n,
  }

  await pic.reinstallCode({
    canisterId: bridgeId,
    wasm: await readFile(path.join(testTarget, "wasm32-unknown-unknown/release/bridge_canister.wasm")),
    arg: IDL.encode([bridgeInitType], [{
      ledger_canister_id: ledgerId,
      index_canister_id: indexId,
      evm_rpc_canister_id: mock.canisterId,
      custom_evm_rpc_urls: [
        "https://one.example",
        "https://two.example",
        "https://three.example",
      ],
      base_chain_id: 31_337n,
      bridge_contract: hexToBytes(bridgeAddress),
      expected_bridge_runtime_sha256: hexToBytes(sha256(bridgeCode)),
      timelock_contract: hexToBytes(timelockAddress),
      deployment_instance_id: new Uint8Array(32).fill(3),
      minimum_withdrawal_id: new Uint8Array([...new Uint8Array(31), 1]),
      ecdsa_key_name: "key_1",
      ecdsa_derivation_path: [],
      governance_ecdsa_derivation_path: [new TextEncoder().encode("governance-operator")],
      deposit_rate_limit_window_seconds: 60n,
      deposit_rate_limit_global: 30,
      deposit_rate_limit_per_principal: 3,
      notification_rate_limit_window_seconds: 600n,
      notification_rate_limit_global: 60,
      notification_ingestion_rate_limit_global: 30,
      settlement_rate_limit_window_seconds: 600n,
      settlement_rate_limit_global: 60,
      settlement_rate_limit_per_principal: 6,
      settlement_rate_limit_per_record: 3,
      settlement_retry_interval_seconds: 60n,
      governance_evm_fee: operationalConfig.governance_evm_fee,
      governance_replacement: {
        max_replacements: 3,
        fee_bump_bps: 1_250,
      },
      cycles_floor: operationalConfig.cycles_floor,
      settlement_cycle_ceiling: operationalConfig.settlement_cycle_ceiling,
      governance_principal: testOwner,
      pause_principal: pausePrincipal,
      confirmation_relayer_principal: confirmationRelayerPrincipal,
      fee_recipient: { owner: feeRecipient, subaccount: [] },
    }]),
    sender: testOwner,
    cycles: 500_000_000_000_000n,
    targetSubnetId: subnet.id,
  })
  const bridgeActor = pic.createActor(bridgeIdl, bridgeId)
  const bridge = { actor: bridgeActor, canisterId: bridgeId }
  bridge.actor.setIdentity(testIdentity)
  const initializedPublicConfig = await bridge.actor.initialize_public_config()
  if (!("Ok" in initializedPublicConfig)) {
    throw new Error(`Failed to initialize public config: ${JSON.stringify(initializedPublicConfig.Err)}`)
  }
  const pauseActor = pic.createActor(bridgeIdl, bridgeId)
  pauseActor.setIdentity(pauseIdentity)
  mock.actor.setIdentity(testIdentity)
  const configuredSigner = await mock.actor.set_bridge_signer_for_canister(bridgeId, "key_1")
  if (!("Ok" in configuredSigner)) throw new Error(`Failed to configure the confirmed bridge signer: ${configuredSigner.Err}`)
  const confirmedSigner = bytesHex(await mock.actor.bridge_signer())
  if (confirmedSigner.toLowerCase() !== signer.toLowerCase()) {
    sendAsTimelock(bridgeAddress, "rotateBridgeSigner(address)", confirmedSigner)
    signer = confirmedSigner
  }
  await rpc("anvil_mine", ["0x40"])
  const initialSafe = await publicClient.getBlock({ blockTag: "safe" })
  if (initialSafe.number === null) throw new Error("Anvil initial safe head is unavailable")
  const initialSafeResult = await mock.actor.set_safe_block(initialSafe.number, hexToBytes(initialSafe.hash))
  if ("Err" in initialSafeResult) throw new Error(initialSafeResult.Err)
  const ledger = pic.createActor(ledgerIdl, ledgerId)
  ledger.setIdentity(testIdentity)
  const index = pic.createActor(indexIdl, indexId)
  const [publicConfig, operationalConfigResult] = await Promise.all([
    bridge.actor.get_runtime_binding(),
    bridge.actor.get_operational_config(),
  ])
  if (!("Ok" in operationalConfigResult)) throw new Error("Bridge operational configuration is unavailable to the controller")
  const liveOperationalConfig = operationalConfigResult.Ok
  if (bytesHex(publicConfig.expected_bridge_signer).toLowerCase() !== signer.toLowerCase()) throw new Error("Bridge mint signer derivation drifted")
  if (bytesHex(liveOperationalConfig.governance_operator).toLowerCase() !== governanceOperator.toLowerCase()) throw new Error("Bridge governance operator derivation drifted")
  const deploymentPostconditions = await mock.actor.set_deployment_postconditions(
    hexToBytes(timelockAddress),
    hexToBytes(governanceOperator),
    hexToBytes(bsnsAddress),
    hexToBytes(bridgeAddress),
    hexToBytes(timelockCode),
    hexToBytes(bsnsCode),
  )
  if (!("Ok" in deploymentPostconditions)) throw new Error(`Failed to configure deployment postconditions: ${deploymentPostconditions.Err}`)
  const governanceReceiptFixture = await mock.actor.set_observed_transaction(
    new Uint8Array(32).fill(9),
    hexToBytes(timelockAddress),
    hexToBytes(governanceOperator),
    1,
  )
  if ("Err" in governanceReceiptFixture) throw new Error(governanceReceiptFixture.Err)
  await pic.advanceTime(2_000)
  await pic.tick(30)

  const gatewayPort = await pic.client.startHttpGateway()
  resources.gatewayClient = pic.client
  await startProgressLoop(pic)
  await writeProfile({
    gatewayPort,
    ledgerId: ledgerId.toText(),
    indexId: indexId.toText(),
    bridgeId: bridge.canisterId.toText(),
    deploymentInstanceId: bytesHex(publicConfig.deployment_instance_id),
    minimumWithdrawalId: bytesHex(publicConfig.minimum_withdrawal_id),
    evmRpcCanisterId: publicConfig.evm_rpc_canister_id.toText(),
    rpcProviderUrlsSha256: bytesHex(publicConfig.rpc_provider_urls_sha256),
    bridgeAddress,
    bsnsAddress,
    timelockAddress,
    expected_bridge_signer: signer,
    deploymentBlock,
    bridgeHash: sha256(bridgeCode),
    bsnsHash: sha256(bsnsCode),
  })

  let relayedBroadcasts = 0
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
      mock.actor.set_block_timestamp(finalized.timestamp),
    ])
    if ("Err" in safeResult) throw new Error(safeResult.Err)
    if ("Err" in finalizedResult) throw new Error(finalizedResult.Err)
    const snapshot = await publicClient.readContract({
      address: bridgeAddress,
      abi: bridgeAbi,
      functionName: "bridgeSnapshot",
    })
    await Promise.all([
      mock.actor.set_deposit_mints_paused(snapshot.depositMintsPaused),
      mock.actor.set_withdrawals_paused(snapshot.withdrawalsPaused),
      mock.actor.set_mint_authorization_epoch(snapshot.mintAuthorizationEpoch),
    ])
  }
  const prepareLatestWithdrawal = async () => {
    const logs = await publicClient.getContractEvents({
      address: bridgeAddress,
      abi: bridgeAbi,
      eventName: "WithdrawalCommitted",
      fromBlock: deploymentBlock,
    })
    const created = logs.at(-1)
    if (!created?.transactionHash) throw new Error("WithdrawalCommitted log is unavailable")
    const receipt = await publicClient.getTransactionReceipt({ hash: created.transactionHash })
    await mock.actor.set_withdrawal([{
      id: hexToBytes(numberToHex(created.args.withdrawalId, { size: 32 })),
      owner: hexToBytes(created.args.owner),
      subaccount: hexToBytes(created.args.subaccount),
      amount: created.args.amount,
      max_service_fee: created.args.maxServiceFee,
      charged_service_fee: created.args.chargedServiceFee,
      amount_out: created.args.amountOut,
    }])
    const observed = await mock.actor.set_observed_transaction(
      hexToBytes(created.transactionHash),
      hexToBytes(bridgeAddress),
      hexToBytes(created.args.requester),
      Number(receipt.blockNumber),
    )
    if ("Err" in observed) throw new Error(observed.Err)
    const withdrawalId = hexToBytes(numberToHex(created.args.withdrawalId, { size: 32 }))
    if (!knownWithdrawals.some((id) => bytesHex(id) === bytesHex(withdrawalId))) knownWithdrawals.push(withdrawalId)
    await syncObservedHeads()
    return created.transactionHash
  }
  const prepareRefundableDeposit = async () => {
    const grossAmount = 100_000_000n
    const ledgerFee = 10_000n
    const ownerSequence = await bridge.actor.get_next_deposit_sequence(testOwner)
    const now = await pic.getTime()
    const approval = await ledger.icrc2_approve({
      from_subaccount: [],
      spender: account(bridge.canisterId),
      amount: grossAmount + ledgerFee,
      expected_allowance: [],
      expires_at: [BigInt(now + 30 * 60 * 1_000) * 1_000_000n],
      fee: [ledgerFee],
      memo: [],
      created_at_time: [],
    })
    if ("Err" in approval) throw new Error(`refund fixture approval failed: ${json(approval.Err)}`)
    await syncObservedHeads()
    const admitted = await bridge.actor.request_deposit({
      owner_sequence: ownerSequence,
      base_recipient: hexToBytes(deployer),
      from_subaccount: [],
      gross_amount: grossAmount,
      max_service_fee: 1_000_000n,
    })
    if ("Err" in admitted) throw new Error(`refund fixture deposit failed: ${json(admitted.Err)}`)
    if (!knownDeposits.some((id) => bytesHex(id) === bytesHex(admitted.Ok.deposit_id))) knownDeposits.push(admitted.Ok.deposit_id)
    depositSequences.push(ownerSequence.toString())

    let record
    const signingDeadline = Date.now() + 60_000
    while (Date.now() < signingDeadline) {
      record = (await bridge.actor.get_deposit(admitted.Ok.deposit_id))[0]
      if (record?.mint_authorization[0]?.signature.length === 1) break
      await delay(250)
    }
    const authorization = record?.mint_authorization[0]
    if (!authorization?.signature.length) throw new Error("refund fixture did not reach a signed Mint Authorization")
    const latest = await publicClient.getBlock({ blockTag: "latest" })
    const advanceSeconds = authorization.deadline >= latest.timestamp
      ? authorization.deadline - latest.timestamp + 1n
      : 1n
    await rpc("evm_increaseTime", [Number(advanceSeconds)])
    await rpc("evm_mine", [])
    await rpc("anvil_mine", ["0x40"])
    await syncObservedHeads()
    const finalized = await publicClient.getBlock({ blockTag: "finalized" })
    if (finalized.timestamp <= authorization.deadline) throw new Error("refund fixture did not pass the strict finalized deadline")
    await pic.advanceTime(60_001)
    return {
      depositId: bytesHex(admitted.Ok.deposit_id),
      ownerSequence: ownerSequence.toString(),
    }
  }
  await syncObservedHeads()
  const relaySigned = async (artifact) => {
    const raw = bytesHex(artifact.raw_transaction)
    const expectedHash = keccak256(raw)
    if (expectedHash.toLowerCase() !== bytesHex(artifact.transaction_hash).toLowerCase()) throw new Error("Canister signed hash mismatch")
    const rawSigner = await recoverTransactionAddress({ serializedTransaction: raw })
    if (rawSigner.toLowerCase() !== governanceOperator.toLowerCase()) {
      throw new Error(`Canister signature used non-Governance signer ${rawSigner}`)
    }
    try {
      const submittedHash = await rpc("eth_sendRawTransaction", [raw])
      if (submittedHash.toLowerCase() !== expectedHash.toLowerCase()) throw new Error("Anvil returned a different raw transaction hash")
    } catch (error) {
      if (!String(error).includes("already known")) throw error
    }
    relayedBroadcasts += 1
    await rpc("evm_mine", [])
    // Anvil's Safe tag trails the tip. Mine enough descendants so receipts
    // relayed in this round can be confirmed by the canister's Safe checks.
    await rpc("anvil_mine", ["0x40"])
    if (!await rpc("eth_getTransactionReceipt", [expectedHash])) throw new Error(`Anvil did not mine relayed transaction ${expectedHash}`)
  }
  const confirmSigned = async (artifact) => {
    await relaySigned(artifact)
    await syncObservedHeads()
    return bridge.actor.confirm_base_governance_transaction({
      operation_id: artifact.operation_id,
      transaction_hash: artifact.transaction_hash,
    })
  }
  await syncObservedHeads()
  const sealedLifecycle = await bridge.actor.seal_operational_config(operationalConfig)
  if (!("Ok" in sealedLifecycle) || !("OperationalConfigSealed" in sealedLifecycle.Ok.lifecycle)) {
    throw new Error(`Failed to seal operational config: ${json(sealedLifecycle)}`)
  }
  const scheduleSubmitted = await bridge.actor.schedule_activation()
  if (!("Ok" in scheduleSubmitted)) throw new Error(`Canister schedule_activation failed: ${json(scheduleSubmitted.Err)}`)
  const scheduleConfirmed = await confirmSigned(scheduleSubmitted.Ok)
  if (!("Ok" in scheduleConfirmed)) throw new Error(`Canister schedule confirmation failed: ${json(scheduleConfirmed.Err)}`)

  const earlyExecuteSubmitted = await bridge.actor.execute_activation()
  if (!("Ok" in earlyExecuteSubmitted)) throw new Error(`Canister early execute submission failed: ${json(earlyExecuteSubmitted.Err)}`)
  await mock.actor.set_receipt_mode({ Reverted: null })
  const earlyExecuteConfirmed = await confirmSigned(earlyExecuteSubmitted.Ok)
  if (!("Err" in earlyExecuteConfirmed) || !("TransactionReverted" in earlyExecuteConfirmed.Err)) {
    throw new Error(`Canister did not record the pre-delay execute revert: ${json(earlyExecuteConfirmed)}`)
  }
  await mock.actor.set_receipt_mode({ Confirmed: null })

  await rpc("evm_increaseTime", [ACTIVATION_DELAY_SECONDS])
  await rpc("evm_mine", [])
  const executeSubmitted = await bridge.actor.execute_activation()
  if (!("Ok" in executeSubmitted)) throw new Error(`Canister execute_activation failed: ${json(executeSubmitted.Err)}`)
  const executeConfirmed = await confirmSigned(executeSubmitted.Ok)
  if (!("Ok" in executeConfirmed)) throw new Error(`Canister execute confirmation failed: ${json(executeConfirmed.Err)}`)
  const activeStatus = await bridge.actor.get_bridge_status()
  if (activeStatus.deposits_paused) throw new Error("IC deposits remained paused after canonical activation")

  const localPause = await pauseActor.pause_new_deposits()
  if (!("Ok" in localPause)) throw new Error(`Pause principal could not pause IC deposits: ${json(localPause.Err)}`)
  const pauseDepositSubmitted = await bridge.actor.prepare_base_governance_action({ PauseDepositMints: null })
  if (!("Ok" in pauseDepositSubmitted)) throw new Error(`Canister deposit pause failed: ${json(pauseDepositSubmitted.Err)}`)
  const pauseDepositConfirmed = await confirmSigned(pauseDepositSubmitted.Ok)
  if (!("Ok" in pauseDepositConfirmed)) throw new Error(`Canister deposit pause confirmation failed: ${json(pauseDepositConfirmed.Err)}`)
  const pauseWithdrawalSubmitted = await bridge.actor.prepare_base_governance_action({ PauseWithdrawals: null })
  if (!("Ok" in pauseWithdrawalSubmitted)) throw new Error(`Canister withdrawal pause failed: ${json(pauseWithdrawalSubmitted.Err)}`)
  const pauseWithdrawalConfirmed = await confirmSigned(pauseWithdrawalSubmitted.Ok)
  if (!("Ok" in pauseWithdrawalConfirmed)) throw new Error(`Canister withdrawal pause confirmation failed: ${json(pauseWithdrawalConfirmed.Err)}`)

  const secondScheduleSubmitted = await bridge.actor.schedule_activation()
  if (!("Ok" in secondScheduleSubmitted)) throw new Error(`Second activation schedule failed: ${json(secondScheduleSubmitted.Err)}`)
  if (secondScheduleSubmitted.Ok.operation_id === scheduleSubmitted.Ok.operation_id) throw new Error("Activation reused its governance operation ID")
  const secondScheduleConfirmed = await confirmSigned(secondScheduleSubmitted.Ok)
  if (!("Ok" in secondScheduleConfirmed)) throw new Error(`Second activation schedule confirmation failed: ${json(secondScheduleConfirmed.Err)}`)
  await rpc("evm_increaseTime", [ACTIVATION_DELAY_SECONDS])
  await rpc("evm_mine", [])
  const secondExecuteSubmitted = await bridge.actor.execute_activation()
  if (!("Ok" in secondExecuteSubmitted)) throw new Error(`Second activation execute failed: ${json(secondExecuteSubmitted.Err)}`)
  const emergencyDuringExecute = await pauseActor.emergency_pause()
  if (!("Ok" in emergencyDuringExecute)) throw new Error(`Emergency pause failed: ${json(emergencyDuringExecute)}`)
  const secondExecuteConfirmed = await confirmSigned(secondExecuteSubmitted.Ok)
  if (!("Ok" in secondExecuteConfirmed)) throw new Error(`Second activation execute confirmation failed: ${json(secondExecuteConfirmed.Err)}`)
  const emergencyStatus = await bridge.actor.get_bridge_status()
  if (!emergencyStatus.deposits_paused) throw new Error("Submitted activation resumed IC deposits during an emergency")

  const emergencyDepositPauseSubmitted = await bridge.actor.prepare_next_emergency_base_action()
  if (!("Ok" in emergencyDepositPauseSubmitted)) throw new Error(`Emergency deposit pause failed: ${json(emergencyDepositPauseSubmitted.Err)}`)
  const emergencyDepositPauseConfirmed = await confirmSigned(emergencyDepositPauseSubmitted.Ok)
  if (!("Ok" in emergencyDepositPauseConfirmed)) throw new Error(`Emergency deposit pause confirmation failed: ${json(emergencyDepositPauseConfirmed.Err)}`)
  const emergencyWithdrawalPauseSubmitted = await bridge.actor.prepare_next_emergency_base_action()
  if (!("Ok" in emergencyWithdrawalPauseSubmitted)) throw new Error(`Emergency withdrawal pause failed: ${json(emergencyWithdrawalPauseSubmitted.Err)}`)
  const emergencyWithdrawalPauseConfirmed = await confirmSigned(emergencyWithdrawalPauseSubmitted.Ok)
  if (!("Ok" in emergencyWithdrawalPauseConfirmed)) throw new Error(`Emergency withdrawal pause confirmation failed: ${json(emergencyWithdrawalPauseConfirmed.Err)}`)

  const recoveryScheduleSubmitted = await bridge.actor.schedule_activation()
  if (!("Ok" in recoveryScheduleSubmitted)) throw new Error(`Post-emergency activation schedule failed: ${json(recoveryScheduleSubmitted.Err)}`)
  const recoveryScheduleConfirmed = await confirmSigned(recoveryScheduleSubmitted.Ok)
  if (!("Ok" in recoveryScheduleConfirmed)) throw new Error(`Post-emergency activation schedule confirmation failed: ${json(recoveryScheduleConfirmed.Err)}`)
  await rpc("evm_increaseTime", [ACTIVATION_DELAY_SECONDS])
  await rpc("evm_mine", [])
  const recoveryExecuteSubmitted = await bridge.actor.execute_activation()
  if (!("Ok" in recoveryExecuteSubmitted)) throw new Error(`Post-emergency activation execute failed: ${json(recoveryExecuteSubmitted.Err)}`)
  const recoveryExecuteConfirmed = await confirmSigned(recoveryExecuteSubmitted.Ok)
  if (!("Ok" in recoveryExecuteConfirmed)) throw new Error(`Post-emergency activation execute confirmation failed: ${json(recoveryExecuteConfirmed.Err)}`)
  const reactivatedStatus = await bridge.actor.get_bridge_status()
  if (reactivatedStatus.deposits_paused) throw new Error("IC deposits remained paused after post-emergency activation")
  // The mock RPC exposes one nonce response at a time. Switch its observation
  // from the governance operator lane to the independently derived mint lane.
  await mock.actor.set_next_evm_nonce(0n)
  resources.activationFacts = {
    schedule_transaction: bytesHex(scheduleSubmitted.Ok.transaction_hash),
    early_execute_reverted: true,
    delay_seconds: ACTIVATION_DELAY_SECONDS,
    execute_transaction: bytesHex(executeSubmitted.Ok.transaction_hash),
    repeated_schedule_transaction: bytesHex(secondScheduleSubmitted.Ok.transaction_hash),
    repeated_execute_transaction: bytesHex(secondExecuteSubmitted.Ok.transaction_hash),
    emergency_resume_suppressed: true,
    recovery_schedule_transaction: bytesHex(recoveryScheduleSubmitted.Ok.transaction_hash),
    recovery_execute_transaction: bytesHex(recoveryExecuteSubmitted.Ok.transaction_hash),
  }
  await writeLocalFacts({
    mint_signer: signer,
    governance_operator: governanceOperator,
    timelock: timelockAddress,
    bridge: bridgeAddress,
    activation: resources.activationFacts,
    state_upgrade: false,
  })
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
        await syncObservedHeads()
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
        return send(response, 200, { deposit_id: bytesHex(result.Ok.deposit_id), owner_sequence: result.Ok.owner_sequence.toString(), state: result.Ok.state })
      }
      if (request.url === "/ic/request-deposit-refund") {
        await syncObservedHeads()
        const result = await bridge.actor.request_deposit_refund(hexToBytes(body.id))
        if ("Err" in result) throw new Error(`refund claim rejected: ${json(result.Err)}`)
        return send(response, 200, result.Ok)
      }
      if (request.url === "/ic/continue-withdrawal") {
        const result = await bridge.actor.continue_withdrawal(hexToBytes(body.id))
        if ("Err" in result) throw new Error(`withdrawal continuation rejected: ${json(result.Err)}`)
        return send(response, 200, settlementJson(result.Ok))
      }
      if (request.url === "/test/prepare-latest-withdrawal") {
        return send(response, 200, { transactionHash: await prepareLatestWithdrawal() })
      }
      if (request.url === "/test/prepare-refundable-deposit") {
        return send(response, 200, await prepareRefundableDeposit())
      }
      if (request.url === "/test/fail-next-deposit-response") {
        failNextDepositResponse = true
        return send(response, 200, null)
      }
      if (request.url === "/test/set-ledger-available") {
        if (body.available) await pic.startCanister({ canisterId: ledgerId })
        else await pic.stopCanister({ canisterId: ledgerId })
        return send(response, 200, null)
      }
      if (request.url === "/test/settle") {
        await syncObservedHeads()
        for (let round = 0; round < 10; round += 1) {
          await pic.advanceTime(60_001)
          await pic.tick(30)
          for (const id of knownWithdrawals) await bridge.actor.continue_withdrawal(id)
        }
        return send(response, 200, null)
      }
      if (request.url === "/test/relay") {
        await syncObservedHeads()
        await stopProgressLoop()
        try {
          await pic.advanceTime(2_000)
          await pic.tick(30)
        } finally {
          await startProgressLoop(pic)
        }
        return send(response, 200, null)
      }
      if (request.url === "/test/upgrade") {
        await stopProgressLoop()
        let before
        let after
        try {
          await waitForNoLeasedJobs(bridge.actor)
          before = await captureUpgradeState(bridge.actor, testOwner, knownDeposits, knownWithdrawals)
          await pic.upgradeCanister({
            canisterId: bridge.canisterId,
            wasm: await readFile(path.join(testTarget, "wasm32-unknown-unknown/release/bridge_canister.wasm")),
            arg: IDL.encode([], []),
            sender: testOwner,
          })
          after = await captureUpgradeState(bridge.actor, testOwner, knownDeposits, knownWithdrawals)
        } finally {
          await startProgressLoop(pic)
        }
        if (json(before) !== json(after)) throw new Error("same-Wasm upgrade changed durable bridge state")
        if (before.deposits.length === 0) throw new Error("same-Wasm upgrade did not exercise an individual Deposit record")
        const facts = JSON.parse(await readFile(path.join(runtimeDir, "local-e2e-facts.json"), "utf8"))
        await writeLocalFacts({
          ...facts,
          state_upgrade: {
            verified: true,
            before,
            after,
          },
        })
        return send(response, 200, { before, after })
      }
      if (request.url === "/test/account") {
        connectedAccount = String(body.owner)
        return send(response, 200, null)
      }
      if (request.url === "/test/latest-withdrawal-state") {
        const id = knownWithdrawals.at(-1)
        if (!id) throw new Error("test withdrawal is unavailable")
        const record = (await bridge.actor.get_withdrawal(id))[0]
        if (!record) throw new Error("test withdrawal record is unavailable")
        return send(response, 200, {
          phase: Object.keys(record.state)[0],
          stopReason: record.last_settlement_stop_reason[0] ?? null,
        })
      }
      if (request.url === "/test/state") {
        const balance = await publicClient.readContract({ address: bsnsAddress, abi: bsnsAbi, functionName: "balanceOf", args: [deployer] })
        const [allowance, ledgerBalance, ledgerFee, indexBalance, indexLedgerId, indexStatus, nextDepositSequence, bridgeStatus, receiptCalls] = await Promise.all([
          publicClient.readContract({ address: bsnsAddress, abi: bsnsAbi, functionName: "allowance", args: [deployer, bridgeAddress] }),
          ledger.icrc1_balance_of(account(testOwner)),
          ledger.icrc1_fee(),
          index.icrc1_balance_of(account(testOwner)),
          index.ledger_id(),
          index.status(),
          bridge.actor.get_next_deposit_sequence(testOwner),
          bridge.actor.get_bridge_status(),
          mock.actor.receipt_call_count(),
        ])
        return send(response, 200, {
          broadcasts: relayedBroadcasts,
          withdrawalCount: bridgeStatus.counts.withdrawals.toString(),
          receiptCalls: receiptCalls.toString(),
          knownDepositCount: knownDeposits.length,
          depositSequences,
          nextDepositSequence: nextDepositSequence.toString(),
          bsnsBalance: balance.toString(),
          bsnsAllowance: allowance.toString(),
          ledgerBalance: ledgerBalance.toString(),
          ledgerFee: ledgerFee.toString(),
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
  await terminateChild(resources.vite, "Vite")
  if (resources.control?.listening) await new Promise((resolve) => resources.control.close(resolve))
  await stopProgressLoop()
  await resources.gatewayClient?.stopHttpGateway().catch(() => undefined)
  await resources.pic?.tearDown().catch(() => undefined)
  await resources.picServer?.stop().catch(() => undefined)
  await terminateChild(resources.anvil, "Anvil")
}

async function captureUpgradeState(actor, owner, depositIds, withdrawalIds) {
  const [status, runtimeBinding, operationalConfig, ownerSequence, deposits, withdrawals, auditPage, activationStatus, storageIntegrity] = await Promise.all([
    actor.get_bridge_status(),
    actor.get_runtime_binding(),
    actor.get_operational_config(),
    actor.get_next_deposit_sequence(owner),
    Promise.all(depositIds.map((id) => actor.get_deposit(id))),
    Promise.all(withdrawalIds.map((id) => actor.get_withdrawal(id))),
    readAllAuditEvents(actor),
    actor.get_activation_status(),
    actor.storage_integrity_check(),
  ])
  if (deposits.some((item) => item.length !== 1) || withdrawals.some((item) => item.length !== 1)) {
    throw new Error("upgrade evidence could not reopen every known settlement record")
  }
  if (!("Ok" in operationalConfig)) throw new Error("upgrade evidence could not read operational configuration")
  if (!("Ok" in activationStatus)) throw new Error(`upgrade evidence could not read activation status: ${json(activationStatus.Err)}`)
  if (storageIntegrity.Ok !== "ok") throw new Error(`upgrade evidence failed storage integrity: ${json(storageIntegrity)}`)
  const durableStatus = {
    schema_version: status.schema_version,
    counts: status.counts,
    deposits_paused: status.deposits_paused,
    withdrawal_fee_guard_active: status.withdrawal_fee_guard_active,
    withdrawal_fee_guard_ledger_fee: status.withdrawal_fee_guard_ledger_fee,
    withdrawal_fee_guard_charged_service_fee: status.withdrawal_fee_guard_charged_service_fee,
    unpaid_withdrawal_count: status.unpaid_withdrawal_count,
    unpaid_withdrawal_amount_out: status.unpaid_withdrawal_amount_out,
    withdrawal_stop_reasons: status.withdrawal_stop_reasons,
    observed_bridge_signer: status.observed_bridge_signer,
    observed_bridge_runtime_sha256: status.observed_bridge_runtime_sha256,
    last_finalized_base_block: status.last_finalized_base_block,
    last_finalized_base_block_hash: status.last_finalized_base_block_hash,
    settlement_scheduler: {
      scheduled: status.settlement_scheduler.scheduled,
      stopped: status.settlement_scheduler.stopped,
      leased: status.settlement_scheduler.leased,
      expired: status.settlement_scheduler.expired,
      health: status.settlement_scheduler.health,
      last_internal_error: status.settlement_scheduler.last_internal_error,
    },
    last_audit_sequence: status.last_audit_sequence,
  }
  return JSON.parse(json({
    status: durableStatus,
    runtime_binding: runtimeBinding,
    operational_config: operationalConfig.Ok,
    owner_sequence: ownerSequence,
    deposits: deposits.map(([item]) => item),
    withdrawals: withdrawals.map(([item]) => item),
    audit_events: auditPage,
    activation_status: activationStatus.Ok,
    storage_integrity: storageIntegrity.Ok,
  }))
}

async function waitForNoLeasedJobs(actor) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const status = await actor.get_bridge_status()
    if (status.settlement_scheduler.leased === 0n) return
    await delay(50)
  }
  throw new Error("settlement scheduler did not release active leases before upgrade")
}

async function readAllAuditEvents(actor) {
  let cursor = 0n
  let metadata
  const events = []
  for (let pageIndex = 0; pageIndex < 10_000; pageIndex += 1) {
    const result = await actor.get_audit_events(cursor, 100)
    if (!("Ok" in result)) throw new Error(`upgrade evidence could not read audit events: ${json(result.Err)}`)
    const page = result.Ok
    metadata ??= {
      pruned_digest: page.pruned_digest,
      oldest_available_sequence: page.oldest_available_sequence,
      pruned_count: page.pruned_count,
      pruned_through_sequence: page.pruned_through_sequence,
    }
    events.push(...page.events)
    const [next] = page.next_sequence
    if (next === undefined) return { ...metadata, events, next_sequence: [] }
    if (next <= cursor) throw new Error("upgrade evidence audit pagination did not advance")
    cursor = next
  }
  throw new Error("upgrade evidence audit pagination exceeded its safety bound")
}

async function terminateChild(child, label) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return
  const exited = new Promise((resolve) => child.once("close", resolve))
  child.kill("SIGTERM")
  const timedOut = await Promise.race([exited.then(() => false), delay(5_000).then(() => true)])
  if (timedOut && child.exitCode === null && child.signalCode === null) {
    child.kill("SIGKILL")
    await exited
  }
  if (child.exitCode === null && child.signalCode === null) throw new Error(`${label} did not terminate`)
}

async function startProgressLoop(pic) {
  await stopProgressLoop()
  await pic.client.autoProgress()
  resources.progressClient = pic.client
}

async function stopProgressLoop() {
  if (resources.progressClient) await resources.progressClient.stopProgress()
  resources.progressClient = undefined
}

function buildWasm() {
  execFileSync("cargo", ["build", "--target", "wasm32-unknown-unknown", "--release", "-p", "bridge-canister", "--features", "test-deployment"], { cwd: root, stdio: "inherit", env: { ...process.env, CARGO_TARGET_DIR: testTarget } })
  execFileSync("cargo", ["build", "--target", "wasm32-unknown-unknown", "--release", "-p", "mock-external"], { cwd: root, stdio: "inherit" })
}

function deployTimelock(governanceOperator) {
  const output = execFileSync("forge", [
    "create", "--root", path.join(root, "contracts"), "--rpc-url", rpcUrl,
    "--from", deployer, "--unlocked", "--broadcast",
    "src/BridgeTimelockController.sol:BridgeTimelockController", "--constructor-args",
    String(ACTIVATION_DELAY_SECONDS),
    `[${governanceOperator}]`,
    `[${governanceOperator}]`,
    `[${governanceOperator}]`,
  ], { encoding: "utf8", env: stagingForgeEnv })
  const match = output.match(/Deployed to:\s*(0x[0-9a-fA-F]{40})/)
  if (!match) throw new Error(`Unable to parse Timelock deployment:\n${output}`)
  return match[1]
}

function deployBridge(signer, governanceOperator, timelockAddress) {
  const timelockCodeHash = execFileSync(
    "cast", ["codehash", timelockAddress, "--rpc-url", rpcUrl], { encoding: "utf8" },
  ).trim()
  const output = execFileSync("forge", [
    "create", "--root", path.join(root, "contracts"), "--rpc-url", rpcUrl,
    "--from", deployer, "--unlocked", "--broadcast", "src/Bridge.sol:Bridge", "--constructor-args",
    signer, governanceOperator, timelockAddress, timelockCodeHash,
    "1000000000000", "10000000000000", "3600", "100000000", "1000000",
  ], { encoding: "utf8", env: stagingForgeEnv })
  const match = output.match(/Deployed to:\s*(0x[0-9a-fA-F]{40})/)
  if (!match) throw new Error(`Unable to parse Bridge deployment:\n${output}`)
  assertFrozenCanisterRoles(match[1], timelockAddress, governanceOperator)
  return match[1]
}

function assertFrozenCanisterRoles(bridgeAddress, timelockAddress, governanceOperator) {
  const runtime = execFileSync("cast", ["call", bridgeAddress, "runtimeAdministrator()(address)", "--rpc-url", rpcUrl], { encoding: "utf8" }).trim()
  if (runtime.toLowerCase() !== governanceOperator.toLowerCase()) throw new Error("Bridge runtime administrator is not the canister governance operator")
  for (const role of [
    execFileSync("cast", ["call", timelockAddress, "PROPOSER_ROLE()(bytes32)", "--rpc-url", rpcUrl], { encoding: "utf8" }).trim(),
    execFileSync("cast", ["call", timelockAddress, "EXECUTOR_ROLE()(bytes32)", "--rpc-url", rpcUrl], { encoding: "utf8" }).trim(),
    execFileSync("cast", ["call", timelockAddress, "CANCELLER_ROLE()(bytes32)", "--rpc-url", rpcUrl], { encoding: "utf8" }).trim(),
  ]) {
    const operatorHasRole = execFileSync("cast", ["call", timelockAddress, "hasRole(bytes32,address)(bool)", role, governanceOperator, "--rpc-url", rpcUrl], { encoding: "utf8" }).trim()
    const deployerHasRole = execFileSync("cast", ["call", timelockAddress, "hasRole(bytes32,address)(bool)", role, deployer, "--rpc-url", rpcUrl], { encoding: "utf8" }).trim()
    if (operatorHasRole !== "true" || deployerHasRole !== "false") throw new Error("Timelock role set is not canister-only")
  }
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
  environmentMode: "short-delay-test-only" | null; activationTimelockDelaySeconds: number | null;
  icHost: string; baseRpcUrl: string; chainId: number; bridgeCanisterId: string | null; ledgerCanisterId: string | null; indexCanisterId: string | null;
  deploymentInstanceId: \`0x\${string}\` | null;
  minimumWithdrawalId: \`0x\${string}\` | null;
  evmRpcCanisterId: string | null; rpcProviderUrlsSha256: \`0x\${string}\` | null;
  icToken: { name: string; symbol: string; decimals: number }; baseToken: { symbol: string; decimals: number };
  bridgeAddress: \`0x\${string}\` | null; bsnsAddress: \`0x\${string}\` | null; timelockAddress: \`0x\${string}\` | null; expected_bridge_signer: \`0x\${string}\` | null; deploymentBlock: bigint | null;
  bridgeRuntimeHash: \`0x\${string}\` | null; bsnsRuntimeHash: \`0x\${string}\` | null;
}
export const deploymentProfile: DeploymentProfile = ${serialize({
    environment: "local-real-e2e", label: "Local Anvil + PocketIC", testOnly: true,
    environmentMode: "short-delay-test-only", activationTimelockDelaySeconds: ACTIVATION_DELAY_SECONDS,
    icHost: `http://127.0.0.1:${values.gatewayPort}`,
    baseRpcUrl: rpcUrl, chainId: 31337, bridgeCanisterId: values.bridgeId, deploymentInstanceId: values.deploymentInstanceId, minimumWithdrawalId: values.minimumWithdrawalId, ledgerCanisterId: values.ledgerId, indexCanisterId: values.indexId,
    evmRpcCanisterId: values.evmRpcCanisterId, rpcProviderUrlsSha256: values.rpcProviderUrlsSha256,
    icToken: { name: "TEST ICRC1", symbol: "TICRC1", decimals: 8 }, baseToken: { symbol: "KINIC", decimals: 8 },
    bridgeAddress: values.bridgeAddress, bsnsAddress: values.bsnsAddress, timelockAddress: values.timelockAddress, expected_bridge_signer: values.expected_bridge_signer, deploymentBlock: values.deploymentBlock,
    bridgeRuntimeHash: values.bridgeHash, bsnsRuntimeHash: values.bsnsHash,
  })}
export function profileCompleteness(profile: DeploymentProfile): string[] {
  const blockers: string[] = []
  if (!profile.bridgeCanisterId || !profile.deploymentInstanceId || !profile.ledgerCanisterId || !profile.indexCanisterId || !profile.evmRpcCanisterId || !profile.rpcProviderUrlsSha256 || !profile.bridgeAddress || !profile.bsnsAddress || !profile.timelockAddress || !profile.expected_bridge_signer || profile.deploymentBlock === null || !profile.bridgeRuntimeHash || !profile.bsnsRuntimeHash) blockers.push("Deployment profile is incomplete")
  return blockers
}
`
  await writeFile(path.join(runtimeDir, "profile.ts"), source)
}

async function writeLocalFacts(value) {
  await writeFile(path.join(runtimeDir, "local-e2e-facts.json"), `${JSON.stringify(value, null, 2)}\n`)
}

function serialize(value) {
  if (typeof value === "bigint") return `${value}n`
  if (Array.isArray(value)) return `[${value.map(serialize).join(",")}]`
  if (value && typeof value === "object") return `{${Object.entries(value).map(([key, item]) => `${JSON.stringify(key)}:${serialize(item)}`).join(",")}}`
  return JSON.stringify(value)
}

function account(owner) { return { owner, subaccount: [] } }
function bytesHex(bytes) { return `0x${Buffer.from(bytes).toString("hex")}` }
function json(value) {
  return JSON.stringify(value, (_key, item) => {
    if (typeof item === "bigint") return item.toString()
    if (item instanceof Uint8Array) return Array.from(item)
    return item
  })
}
function settlementJson(value) {
  if ("Submitted" in value) return { Submitted: { ...value.Submitted, transaction_hash: Array.from(value.Submitted.transaction_hash) } }
  if ("WaitingForConfirmation" in value) return { WaitingForConfirmation: { ...value.WaitingForConfirmation, transaction_hash: Array.from(value.WaitingForConfirmation.transaction_hash) } }
  return value
}
async function readJson(request) { const chunks = []; for await (const chunk of request) chunks.push(chunk); return JSON.parse(Buffer.concat(chunks).toString("utf8") || "null") }
function send(response, status, value) { response.statusCode = status; response.setHeader("content-type", "application/json"); response.end(value === null ? "null" : json(value)); }
async function rpc(method, params) { const response = await fetch(rpcUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }) }); const value = await response.json(); if (value.error) throw new Error(value.error.message); return value.result }
async function waitForOwnedRpc(child) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (child.exitCode !== null) throw new Error(`spawned Anvil exited before becoming ready (exit ${child.exitCode})`)
    try {
      if (await rpc("eth_chainId", []) === "0x7a69") {
        process.kill(child.pid, 0)
        return
      }
    } catch {}
    await delay(100)
  }
  throw new Error("spawned Anvil did not become ready on its owned port")
}
async function isTcpPortOpen(host, port) {
  return await new Promise((resolve) => {
    const socket = connect({ host, port })
    socket.once("connect", () => { socket.destroy(); resolve(true) })
    socket.once("error", () => { socket.destroy(); resolve(false) })
    socket.setTimeout(500, () => { socket.destroy(); resolve(false) })
  })
}
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
    icrc1_fee: I.Func([], [I.Nat], ["query"]),
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
