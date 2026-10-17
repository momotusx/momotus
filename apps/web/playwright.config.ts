import { defineConfig } from "@playwright/test";
import { PORTS } from "./e2e/core";

export default defineConfig({
  testDir: "e2e",
  timeout: 120_000,
  expect: { timeout: 20_000 },
  // One core is shared by every test, so they run in order.
  workers: 1,
  // Live third-party data (addons, image CDN) has slow moments; one retry,
  // and the report shows any test that needed it.
  retries: 1,
  fullyParallel: false,
  reporter: [["list"]],
  globalSetup: "./e2e/global-setup.ts",
  use: {
    baseURL: `http://127.0.0.1:${PORTS.vite}`,
    colorScheme: "dark",
    trace: "retain-on-failure",
  },
  webServer: {
    command: "bunx vite",
    url: `http://127.0.0.1:${PORTS.vite}`,
    reuseExistingServer: false,
    env: { PORT: String(PORTS.vite), VITE_MQ_BRIDGE: `ws://127.0.0.1:${PORTS.bridge}/ws` },
  },
});
