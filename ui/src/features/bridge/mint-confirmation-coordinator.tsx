import { useEffect } from "react"
import { useQueryClient } from "@tanstack/react-query"
import { readAllPendingMints } from "@/lib/pending-confirmations"
import { observeMint } from "@/lib/mint-observation"

export function MintConfirmationCoordinator() {
  const queryClient = useQueryClient()
  useEffect(() => {
    let active = true
    let running = false
    const completedObservations = new Set<string>()
    const tick = async () => {
      if (!active || running || document.visibilityState !== "visible") return
      running = true
      try {
        for (const pending of readAllPendingMints()) {
          if (!active) break
          if (completedObservations.has(pending.transactionHash)) continue
          const observation = await observeMint(pending)
          queryClient.setQueryData(["mint-observation", pending.depositId], observation)
          if (observation.finalized && observation.status === "reverted")
            completedObservations.add(pending.transactionHash)
          if (observation.recorded) {
            completedObservations.add(pending.transactionHash)
            void queryClient.invalidateQueries({ queryKey: ["deposit-history"] })
          }
        }
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
  }, [queryClient])
  return null
}
