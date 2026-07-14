import path from "node:path"
import { defineConfig } from "vite"
import baseConfig from "./vite.config"

export default defineConfig({
  ...baseConfig,
  resolve: {
    alias: [
      { find: /^@\/config\/profile$/, replacement: path.resolve(import.meta.dirname, ".e2e-runtime/profile.ts") },
      { find: /^@\/features\/wallet\/ic-wallet-provider$/, replacement: path.resolve(import.meta.dirname, "e2e-real/ic-wallet-provider.tsx") },
      { find: "@", replacement: path.resolve(import.meta.dirname, "src") },
    ],
  },
})
