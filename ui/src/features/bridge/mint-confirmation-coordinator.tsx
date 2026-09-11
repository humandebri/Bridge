import { useEffect } from "react"
import { useQueryClient } from "@tanstack/react-query"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import type { MintObservation } from "@/lib/mint-observation"
import { runMintRecoveryCycle } from "@/lib/mint-recovery"

export function MintConfirmationCoordinator() {
  const queryClient = useQueryClient()
  const ic = useIcWallet()
  const owner = (ic.account ?? ic.historyAccount)?.owner
  useEffect(() => {
    let active = true
    let running = false
    const tick = async () => {
      if (!active || running || document.visibilityState !== "visible") return
      running = true
      try {
        const result = await runMintRecoveryCycle(owner)
        if (!active || !result) return
        queryClient.setQueryData(["mint-observation", result.depositId], result.observation)
        queryClient.setQueriesData<Map<string, MintObservation>>(
          { queryKey: ["deposit-mint-observations"] },
          (current) => {
            if (!current?.has(result.depositId)) return current
            const next = new Map(current)
            next.set(result.depositId, result.observation)
            return next
          },
        )
        if (result.observation.recorded)
          void queryClient.invalidateQueries({ queryKey: ["deposit-history"] })
      } catch {
        // The shared scheduler backs off; IC discovery will resume on a later tick.
      } finally {
        running = false
      }
    }
    void tick()
    const timer = window.setInterval(() => void tick(), 10_000)
    document.addEventListener("visibilitychange", tick)
    return () => {
      active = false
      window.clearInterval(timer)
      document.removeEventListener("visibilitychange", tick)
    }
  }, [owner, queryClient])
  return null
}
