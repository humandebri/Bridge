import { useEffect, useEffectEvent } from "react"

export const ACTIVITY_AUTO_REFRESH_INTERVAL_MS = 60_000

export function useActivityAutoRefresh(enabled: boolean, refresh: () => void): void {
  const refreshLatest = useEffectEvent(refresh)

  useEffect(() => {
    if (!enabled) return
    const timer = window.setInterval(refreshLatest, ACTIVITY_AUTO_REFRESH_INTERVAL_MS)
    return () => window.clearInterval(timer)
  }, [enabled])
}
