import path from "node:path"
import { defineConfig } from "vite"
const root = path.resolve(import.meta.dirname, "..")
const environment = path.join(root, "e2e/fixtures/mint-recovery-environment.ts")
export default defineConfig({
  root,
  resolve: {
    alias: [
      ...[
        "@/config/profile",
        "@/lib/evm/client",
        "@/lib/ic/bridge",
        "@/lib/ic/withdrawal-notification-client",
      ].map((find) => ({ find, replacement: environment })),
      { find: "@", replacement: path.join(root, "src") },
    ],
  },
})
