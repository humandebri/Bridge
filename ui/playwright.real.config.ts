import { defineConfig, devices } from "@playwright/test"

export default defineConfig({
  testDir: "./e2e-real",
  testMatch: "bridge-real.spec.ts",
  globalSetup: "./e2e-real/global-setup.mjs",
  fullyParallel: false,
  workers: 1,
  // The real suite mutates one shared PocketIC state, so retrying an individual test is not isolated.
  retries: 0,
  timeout: 180_000,
  expect: { timeout: 30_000 },
  reporter: process.env.CI ? "github" : "list",
  outputDir: "test-results/real",
  use: {
    ...devices["Desktop Chrome"],
    baseURL: "http://127.0.0.1:4174",
    trace: "on-first-retry",
  },
})
