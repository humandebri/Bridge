import { describe, expect, it, vi } from "vitest"
import { finalizedCheckpointMatches } from "./finalized-checkpoint"

const oldHash = `0x${"11".repeat(32)}` as const
const newHash = `0x${"22".repeat(32)}` as const

function input(finalizedBlock: bigint, finalizedBlockHash: `0x${string}`) {
  return {
    finalizedBlock,
    finalizedBlockHash,
    checkpointBlock: 100n,
    checkpointBlockHash: oldHash,
    fetchCheckpointBlockHash: vi.fn().mockResolvedValue(oldHash),
  }
}

describe("finalizedCheckpointMatches", () => {
  it("rejects a checkpoint ahead of the finalized head without an RPC", async () => {
    const args = input(99n, newHash)
    await expect(finalizedCheckpointMatches(args)).resolves.toBe(false)
    expect(args.fetchCheckpointBlockHash).not.toHaveBeenCalled()
  })

  it("uses the already-fetched finalized hash at the same height", async () => {
    const matching = input(100n, oldHash)
    await expect(finalizedCheckpointMatches(matching)).resolves.toBe(true)
    expect(matching.fetchCheckpointBlockHash).not.toHaveBeenCalled()

    const reorged = input(100n, newHash)
    await expect(finalizedCheckpointMatches(reorged)).resolves.toBe(false)
    expect(reorged.fetchCheckpointBlockHash).not.toHaveBeenCalled()
  })

  it("fetches an older checkpoint once after the finalized head advances", async () => {
    const matching = input(101n, newHash)
    await expect(finalizedCheckpointMatches(matching)).resolves.toBe(true)
    expect(matching.fetchCheckpointBlockHash).toHaveBeenCalledOnce()
    expect(matching.fetchCheckpointBlockHash).toHaveBeenCalledWith(100n)

    const reorged = input(101n, newHash)
    reorged.fetchCheckpointBlockHash.mockResolvedValue(newHash)
    await expect(finalizedCheckpointMatches(reorged)).resolves.toBe(false)
    expect(reorged.fetchCheckpointBlockHash).toHaveBeenCalledOnce()
  })
})
