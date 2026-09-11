import { z } from "zod"

export const MINT_RECOVERY_URL = "https://recovery.bridge.kinic.xyz/v1/mint-recovery"
export const recoveryHash = z.string().regex(/^0x[0-9a-f]{64}$/i)
export const recoveryRequestSchema = z
  .object({
    depositId: recoveryHash,
    cursor: z.string().max(3500).optional(),
  })
  .strict()
export const recoveryPageSchema = z
  .object({
    deploymentInstanceId: recoveryHash,
    depositId: recoveryHash,
    authorizationDigest: recoveryHash,
    hashes: z.array(recoveryHash).max(100),
    cursor: z.string().max(3500).nullable(),
  })
  .strict()
export type RecoveryPage = z.infer<typeof recoveryPageSchema>

const diagnostics: { at: number; event: string; candidates: number }[] = []
export function recordMintRecoveryDiagnostic(
  event: "page" | "verified" | "conflict" | "unavailable",
  candidates = 0,
): void {
  diagnostics.push({ at: Date.now(), event, candidates })
  if (diagnostics.length > 100) diagnostics.shift()
}
export function mintRecoveryDiagnostics() {
  return diagnostics.slice()
}
