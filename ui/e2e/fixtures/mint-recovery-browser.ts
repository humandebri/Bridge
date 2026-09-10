import { rememberMintRecovery, runMintRecoveryCycle } from "@/lib/mint-recovery"
import { record, installRecoveryServiceFixture } from "./mint-recovery-environment"
installRecoveryServiceFixture()
const status = document.getElementById("status")!
const writes = document.getElementById("writes")!
const render = () => {
  writes.textContent = `Wallet requests: ${localStorage.getItem("fixture-wallet-requests") ?? "0"}`
  if (localStorage.getItem("fixture-recorded") === "yes")
    status.textContent = "Base success — Recorded on IC"
}
document.getElementById("request")!.onclick = async () => {
  await rememberMintRecovery(record, "aaaaa-aa", true)
  localStorage.setItem(
    "fixture-wallet-requests",
    String(Number(localStorage.getItem("fixture-wallet-requests") ?? 0) + 1),
  )
  status.textContent = "Waiting for wallet response"
  render()
}
document.getElementById("complete")!.onclick = () => {
  localStorage.setItem("fixture-base-success", "yes")
  status.textContent = "Base success — waiting for IC recording"
}
let running = false
setInterval(async () => {
  if (running) return
  running = true
  try {
    const result = await runMintRecoveryCycle("aaaaa-aa")
    if (result?.observation.unavailable)
      status.textContent = "検索サービスに接続できません。再試行します"
    render()
  } catch {
    status.textContent = "Recovery temporarily unavailable; retrying"
  } finally {
    running = false
  }
}, 1000)
render()
