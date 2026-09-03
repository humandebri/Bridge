export async function finalizedCheckpointMatches({
  finalizedBlock,
  finalizedBlockHash,
  checkpointBlock,
  checkpointBlockHash,
  fetchCheckpointBlockHash,
}: {
  finalizedBlock: bigint
  finalizedBlockHash: `0x${string}`
  checkpointBlock: bigint
  checkpointBlockHash: `0x${string}`
  fetchCheckpointBlockHash: (blockNumber: bigint) => Promise<`0x${string}` | null>
}): Promise<boolean> {
  if (finalizedBlock < checkpointBlock) return false
  if (finalizedBlock === checkpointBlock) return finalizedBlockHash === checkpointBlockHash
  return (await fetchCheckpointBlockHash(checkpointBlock)) === checkpointBlockHash
}
