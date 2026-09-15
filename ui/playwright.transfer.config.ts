import { defineConfig } from "@playwright/test"
export default defineConfig({
  testDir: "./e2e",
  testMatch: "transfer-state.spec.ts",
  workers: 1,
  outputDir: "test-results/transfer-state",
  use: {
    browserName: "chromium",
    headless: true,
    screenshot: "only-on-failure",
  },
})
