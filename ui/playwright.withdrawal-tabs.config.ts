import { defineConfig } from "@playwright/test"
export default defineConfig({
  testDir: "./e2e",
  testMatch: "withdrawal-tabs.spec.ts",
  workers: 1,
  outputDir: "test-results/withdrawal-tabs",
  use: { browserName: "chromium", headless: true, screenshot: "only-on-failure" },
})
