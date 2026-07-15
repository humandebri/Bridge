import { Principal } from "@dfinity/principal"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type { DeploymentProfile } from "@/config/profile"
import { bridgeSignerBlockers, CANISTER_SAFE_OBSERVATION_TTL_MS, canisterSafeObservationBlockers, refetchRuntimeWriteReady, requireRuntimeWriteReady, RUNTIME_VALIDATION_TTL_MS, runtimeWriteBlocker, validateRuntime } from "./runtime-validation"

const mocks = vi.hoisted(() => ({
  createPublicClient: vi.fn(),
  createBridgeActor: vi.fn(),
  createIndexActor: vi.fn(),
  createLedgerActor: vi.fn(),
  sha256: vi.fn(),
}))

vi.mock("viem", () => ({
  createPublicClient: mocks.createPublicClient,
  defineChain: (chain: unknown) => chain,
  http: () => ({}),
  sha256: mocks.sha256,
}))
vi.mock("@/lib/ic/bridge", () => ({ createBridgeActor: mocks.createBridgeActor }))
vi.mock("@/lib/ic/index", () => ({ createIndexActor: mocks.createIndexActor }))
vi.mock("@/lib/ic/ledger", () => ({ createLedgerActor: mocks.createLedgerActor }))

const bridgeAddress = `0x${"11".repeat(20)}` as const
const bsnsAddress = `0x${"22".repeat(20)}` as const
const bridgeHash = `0x${"aa".repeat(32)}` as const
const bsnsHash = `0x${"bb".repeat(32)}` as const
const expectedSigner = `0x${"33".repeat(20)}` as const
const safeHash = `0x${"44".repeat(32)}` as const
const ledgerId = "3jkp5-oyaaa-aaaaj-azwqa-cai"
const indexId = "qzre3-3iaaa-aaaai-aqmsa-cai"

const profile: DeploymentProfile = {
  environment: "test",
  label: "Test",
  testOnly: true,
  gateBManifestSha256: null,
  profileFileSha256: "1".repeat(64),
  profileCanonicalSha256: "2".repeat(64),
  icHost: "http://127.0.0.1:4943",
  baseRpcUrl: "http://127.0.0.1:8545",
  chainId: 31_337,
  bridgeCanisterId: "aaaaa-aa",
  ledgerCanisterId: ledgerId,
  indexCanisterId: indexId,
  icToken: { name: "TEST ICRC1", symbol: "TICRC1", decimals: 8 },
  baseToken: { symbol: "KINIC", decimals: 8 },
  bridgeAddress,
  bsnsAddress,
  expected_bridge_signer: expectedSigner,
  evmRpcCanisterId: "7hfb6-caaaa-aaaar-qadga-cai",
  rpcProviderUrlsSha256: `0x${"cc".repeat(32)}`,
  deploymentBlock: 1n,
  bridgeRuntimeHash: bridgeHash,
  bsnsRuntimeHash: bsnsHash,
}

let ledgerMetadata = { name: "TEST ICRC1", symbol: "TICRC1", decimals: 8 }
let configuredLedgerId = ledgerId
let configuredIndexId = indexId
let baseMetadata = { symbol: "KINIC", decimals: 8 }
let indexLedgerId = ledgerId
let contractSigner = expectedSigner
let canisterStatus: ReturnType<typeof validCanisterStatus>
let canonicalObservedBlock = 10n
const getBlockMock = vi.fn()
const getCodeMock = vi.fn()
const readContractMock = vi.fn()

function validCanisterStatus(now = Date.now()) {
  return {
    base_chain_id_matches_config: true,
    last_safe_base_block: 10n,
    last_safe_base_block_hash: new Uint8Array(32).fill(0x44),
    last_safe_observation_ns: BigInt(now) * 1_000_000n,
    observed_base_chain_id: [BigInt(profile.chainId)] as [bigint],
    observed_bridge_signer: new Uint8Array(20).fill(0x33),
    observed_bridge_runtime_sha256: new Uint8Array(32).fill(0xaa),
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  ledgerMetadata = { name: "TEST ICRC1", symbol: "TICRC1", decimals: 8 }
  configuredLedgerId = ledgerId
  configuredIndexId = indexId
  baseMetadata = { symbol: "KINIC", decimals: 8 }
  indexLedgerId = ledgerId
  contractSigner = expectedSigner
  canonicalObservedBlock = 10n
  canisterStatus = validCanisterStatus()
  getBlockMock.mockImplementation(({ blockHash }: { blockHash?: `0x${string}` }) => Promise.resolve(blockHash
      ? { number: canonicalObservedBlock, hash: blockHash }
      : { number: 12n, hash: `0x${"66".repeat(32)}` }))
  getCodeMock.mockImplementation(({ address }: { address: string }) => Promise.resolve(address === bridgeAddress ? "0x01" : "0x02"))
  readContractMock.mockImplementation(({ functionName }: { functionName: string }) => {
    if (functionName === "bridgeSnapshot") return Promise.resolve({ bridgeSigner: contractSigner })
    if (functionName === "bsns") return Promise.resolve(bsnsAddress)
    if (functionName === "symbol") return Promise.resolve(baseMetadata.symbol)
    if (functionName === "decimals") return Promise.resolve(baseMetadata.decimals)
    throw new Error(`Unexpected contract call ${functionName}`)
  })
  mocks.createPublicClient.mockReturnValue({
    getBlock: getBlockMock,
    getChainId: vi.fn().mockResolvedValue(profile.chainId),
    getCode: getCodeMock,
    readContract: readContractMock,
  })
  mocks.sha256.mockImplementation((code: string) => code === "0x01" ? bridgeHash : bsnsHash)
  mocks.createBridgeActor.mockResolvedValue({
    refresh_base_observation: vi.fn().mockResolvedValue({ Ok: null }),
    get_public_config: vi.fn().mockImplementation(() => Promise.resolve({
      base_chain_id: BigInt(profile.chainId),
      bridge_contract: Array.from({ length: 20 }, () => 0x11),
      ledger_canister_id: Principal.fromText(configuredLedgerId),
      index_canister_id: Principal.fromText(configuredIndexId),
      schema_version: 6,
      expected_bridge_signer: new Uint8Array(20).fill(0x33),
      evm_rpc_canister_id: Principal.fromText(profile.evmRpcCanisterId as string),
      rpc_provider_urls_sha256: new Uint8Array(32).fill(0xcc),
    })),
    get_bridge_status: vi.fn().mockImplementation(() => Promise.resolve(canisterStatus)),
  })
  mocks.createLedgerActor.mockResolvedValue({
    icrc1_name: vi.fn().mockImplementation(() => Promise.resolve(ledgerMetadata.name)),
    icrc1_symbol: vi.fn().mockImplementation(() => Promise.resolve(ledgerMetadata.symbol)),
    icrc1_decimals: vi.fn().mockImplementation(() => Promise.resolve(ledgerMetadata.decimals)),
  })
  mocks.createIndexActor.mockResolvedValue({
    ledger_id: vi.fn().mockImplementation(() => Promise.resolve(Principal.fromText(indexLedgerId))),
  })
})

describe("validateRuntime token bindings", () => {
  it("rejects every write until runtime verification is ready", () => {
    expect(() => requireRuntimeWriteReady()).toThrow("Refresh to verify the reviewed deployment")
    expect(() => requireRuntimeWriteReady({ ready: false, blockers: ["Bridge signer differs"], checkedAt: 1 })).toThrow("Bridge signer differs")
    expect(() => requireRuntimeWriteReady({ ready: true, blockers: [], checkedAt: 1 }, 1)).not.toThrow()
  })

  it("expires a successful runtime verification after the write TTL", () => {
    const validation = { ready: true, blockers: [], checkedAt: 10_000 }
    expect(runtimeWriteBlocker(validation, 10_000 + RUNTIME_VALIDATION_TTL_MS)).toBeUndefined()
    expect(runtimeWriteBlocker(validation, 10_001 + RUNTIME_VALIDATION_TTL_MS)).toBe("Runtime verification expired. Refresh before continuing.")
    expect(() => requireRuntimeWriteReady(validation, 10_001 + RUNTIME_VALIDATION_TTL_MS)).toThrow("Runtime verification expired")
  })

  it("uses the action-time refetch result and rejects a rotated signer", async () => {
    const cached = { ready: true, blockers: [], checkedAt: Date.now() }
    const rotated = { ready: false, blockers: ["Bridge signer differs from the reviewed profile"], checkedAt: Date.now() }
    const refetch = vi.fn().mockResolvedValue({ data: rotated })
    expect(runtimeWriteBlocker(cached)).toBeUndefined()
    await expect(refetchRuntimeWriteReady(refetch)).rejects.toThrow("Bridge signer differs from the reviewed profile")
    expect(refetch).toHaveBeenCalledOnce()
  })

  it("detects a signer rotation on a later runtime check", async () => {
    await expect(validateRuntime(profile, profile.chainId)).resolves.toMatchObject({ ready: true })
    contractSigner = `0x${"44".repeat(20)}`
    await expect(validateRuntime(profile, profile.chainId)).resolves.toMatchObject({
      ready: false,
      blockers: ["Bridge signer differs from the reviewed profile"],
    })
  })

  it("accepts the reviewed TICRC1 ledger, index, and KINIC Base token", async () => {
    await expect(validateRuntime(profile, profile.chainId)).resolves.toMatchObject({ ready: true, blockers: [] })
    expect(mocks.sha256).toHaveBeenCalledWith("0x01")
    expect(mocks.sha256).toHaveBeenCalledWith("0x02")
    expect(getCodeMock).toHaveBeenCalledWith(expect.objectContaining({ blockHash: safeHash, requireCanonical: true }))
    expect(readContractMock).toHaveBeenCalledWith(expect.objectContaining({ blockHash: safeHash, requireCanonical: true }))
  })

  it("uses the Canister quorum snapshot even when the browser Safe head is newer", async () => {
    const result = await validateRuntime(profile, profile.chainId)
    expect(result).toMatchObject({ ready: true, blockers: [] })
    expect(getBlockMock).toHaveBeenCalledWith({ blockHash: safeHash })
  })

  it("rejects a Canister observation ahead of the browser RPC Safe head", async () => {
    canisterStatus.last_safe_base_block = 13n
    const result = await validateRuntime(profile, profile.chainId)
    expect(result.blockers).toContain("Canister Safe block is ahead of the configured Base RPC Safe head")
  })

  it("rejects a Canister hash that the browser RPC does not bind to the observed height", async () => {
    canonicalObservedBlock = 9n
    const result = await validateRuntime(profile, profile.chainId)
    expect(result.blockers).toContain("Canister Safe block hash is not canonical on the configured Base RPC")
  })

  it("rejects missing, stale, or future Canister observations", () => {
    const now = 100_000
    const missing = validCanisterStatus(now)
    missing.last_safe_base_block_hash = new Uint8Array()
    missing.observed_bridge_signer = new Uint8Array()
    expect(canisterSafeObservationBlockers(profile, 10n, missing, now)).toEqual(expect.arrayContaining([
      "Canister Safe block observation is unavailable",
      "Canister observed Bridge signer is unavailable",
    ]))

    const stale = validCanisterStatus(now - CANISTER_SAFE_OBSERVATION_TTL_MS - 1)
    expect(canisterSafeObservationBlockers(profile, 10n, stale, now)).toContain("Canister Safe block observation is unavailable or stale")
    const future = validCanisterStatus(now + 5_001)
    expect(canisterSafeObservationBlockers(profile, 10n, future, now)).toContain("Canister Safe block observation is unavailable or stale")
  })

  it("requires the Canister-observed chain, signer, and runtime hash to match the profile", () => {
    const status = validCanisterStatus(100_000)
    status.observed_base_chain_id = [1n]
    status.base_chain_id_matches_config = false
    status.observed_bridge_signer = new Uint8Array(20).fill(0x55)
    status.observed_bridge_runtime_sha256 = new Uint8Array(32).fill(0x66)
    expect(canisterSafeObservationBlockers(profile, 10n, status, 100_000)).toEqual(expect.arrayContaining([
      "Canister observed a different Base chain",
      "Canister observed Bridge signer differs from the reviewed profile",
      "Canister observed Bridge runtime bytecode differs from the reviewed profile",
    ]))
  })

  it("blocks when the Canister EVM RPC binding differs", async () => {
    mocks.createBridgeActor.mockResolvedValue({
      refresh_base_observation: vi.fn().mockResolvedValue({ Ok: null }),
      get_public_config: vi.fn().mockResolvedValue({
        base_chain_id: BigInt(profile.chainId), bridge_contract: new Uint8Array(20).fill(0x11),
        ledger_canister_id: Principal.fromText(ledgerId), index_canister_id: Principal.fromText(indexId),
        schema_version: 6, expected_bridge_signer: new Uint8Array(20).fill(0x33),
        evm_rpc_canister_id: Principal.managementCanister(), rpc_provider_urls_sha256: new Uint8Array(32).fill(0xdd),
      }),
      get_bridge_status: vi.fn().mockImplementation(() => Promise.resolve(canisterStatus)),
    })
    const result = await validateRuntime(profile)
    expect(result.blockers).toContain("Canister EVM RPC ID differs from the profile")
    expect(result.blockers).toContain("Canister RPC provider URLs differ from the profile")
  })

  it("blocks unless the profile, confirmed contract, and canister signer agree", () => {
    expect(bridgeSignerBlockers(expectedSigner, expectedSigner, new Uint8Array(20).fill(0x33))).toEqual([])
    expect(bridgeSignerBlockers(expectedSigner, `0x${"44".repeat(20)}`, new Uint8Array(20).fill(0x33))).toContain("Bridge signer differs from the reviewed profile")
    expect(bridgeSignerBlockers(expectedSigner, expectedSigner, new Uint8Array(20).fill(0x44))).toContain("Canister expected Bridge signer differs from the reviewed profile")
    expect(bridgeSignerBlockers(expectedSigner, expectedSigner)).toContain("Canister expected Bridge signer is unavailable")
  })

  it.each([
    ["name", "OTHER"],
    ["symbol", "KINIC"],
    ["decimals", 6],
  ] as const)("blocks when the IC token %s differs", async (field, value) => {
    ledgerMetadata = { ...ledgerMetadata, [field]: value }
    const result = await validateRuntime(profile)
    expect(result.ready).toBe(false)
    expect(result.blockers).toContain("IC token metadata is not TEST ICRC1/TICRC1/8")
  })

  it("blocks when the Bridge points to another index", async () => {
    configuredIndexId = "aaaaa-aa"
    const result = await validateRuntime(profile)
    expect(result.blockers).toContain("Canister index differs from the profile")
  })

  it("blocks when the Bridge points to another ledger", async () => {
    configuredLedgerId = "aaaaa-aa"
    const result = await validateRuntime(profile)
    expect(result.blockers).toContain("Canister ledger differs from the profile")
  })

  it("blocks when the Index points to another ledger", async () => {
    indexLedgerId = "aaaaa-aa"
    const result = await validateRuntime(profile)
    expect(result.blockers).toContain("Index ledger differs from the profile")
  })

  it("blocks when the Index ledger binding cannot be queried", async () => {
    mocks.createIndexActor.mockResolvedValue({ ledger_id: vi.fn().mockRejectedValue(new Error("unavailable")) })
    const result = await validateRuntime(profile)
    expect(result.blockers).toContain("Index ledger binding is unavailable")
  })

  it.each([
    ["symbol", "OTHER"],
    ["decimals", 18],
  ] as const)("blocks when the Base token %s differs", async (field, value) => {
    baseMetadata = { ...baseMetadata, [field]: value }
    const result = await validateRuntime(profile)
    expect(result.blockers).toContain("Base token metadata is not KINIC/8")
  })
})
