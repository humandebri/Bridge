import { createFileRoute, useNavigate } from "@tanstack/react-router"
import { BridgePage, type BridgeDirection } from "@/features/bridge/bridge-page"

export const Route = createFileRoute("/")({
  validateSearch: (search: Record<string, unknown>): { direction: BridgeDirection } => ({ direction: search.direction === "withdraw" ? "withdraw" : "deposit" }),
  component: IndexPage,
})

function IndexPage() {
  const { direction } = Route.useSearch()
  const navigate = useNavigate({ from: "/" })
  return <BridgePage direction={direction} onDirectionChange={(next) => void navigate({ search: { direction: next }, replace: true })} />
}
