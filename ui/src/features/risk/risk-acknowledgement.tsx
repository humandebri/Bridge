import { useState } from "react"
import { ShieldAlert } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { deploymentProfile } from "@/config/profile"

const STORAGE_PREFIX = "kinic.bridge.risk-acknowledgement.v1"
const ACKNOWLEDGED_VALUE = "acknowledged"

export function riskAcknowledgementStorageKey(): string {
  return [
    STORAGE_PREFIX,
    deploymentProfile.chainId,
    deploymentProfile.bridgeAddress?.toLowerCase() ?? "",
    deploymentProfile.bridgeCanisterId ?? "",
  ].join(":")
}

function readAcknowledgement(): boolean {
  if (typeof window === "undefined") return false
  try {
    return window.localStorage.getItem(riskAcknowledgementStorageKey()) === ACKNOWLEDGED_VALUE
  } catch {
    return false
  }
}

export function persistRiskAcknowledgement(storage: Pick<Storage, "setItem"> = window.localStorage): void {
  try {
    storage.setItem(riskAcknowledgementStorageKey(), ACKNOWLEDGED_VALUE)
  } catch { /* Continue for this page load; a future load will ask again. */ }
}

export function RiskAcknowledgementDialog() {
  const [acknowledged, setAcknowledged] = useState(readAcknowledgement)
  const [checked, setChecked] = useState(false)

  if (acknowledged) return null

  const acknowledge = () => {
    setAcknowledged(true)
    persistRiskAcknowledgement()
  }

  return <Dialog open onOpenChange={() => undefined}>
    <DialogContent dismissible={false} className="max-w-[520px] p-5 sm:p-7">
      <DialogHeader className="pr-0">
        <div className="mb-2 flex size-12 items-center justify-center rounded-2xl bg-[#fff0f2] text-[var(--danger)]">
          <ShieldAlert className="size-6" aria-hidden="true" />
        </div>
        <DialogTitle>Unaudited bridge</DialogTitle>
        <DialogDescription>This bridge has not been audited. Bugs or attacks may result in the partial or total loss of funds. Use at your own risk.</DialogDescription>
      </DialogHeader>
      <label className="mt-6 flex cursor-pointer items-start gap-3 rounded-2xl border border-[var(--line)] bg-[var(--panel)] p-4 text-sm leading-6 text-[var(--ink)]">
        <Checkbox
          aria-label="Acknowledge unaudited bridge risk"
          checked={checked}
          onCheckedChange={(value) => setChecked(value === true)}
          className="mt-0.5"
        />
        <span>I understand that this bridge is unaudited and that I may lose my funds.</span>
      </label>
      <DialogFooter>
        <Button className="w-full sm:w-auto" disabled={!checked} onClick={acknowledge}>Acknowledge and continue</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
}
