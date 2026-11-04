import { defineConfig } from "@playwright/test";

// Runs against the in-process system (crates/testkit/src/bin/rcli-devstack.rs): real Server on Postgres in a
// test container + FakePlatform Agents, with a manual clock. Needs Docker (export DOCKER_HOST for Colima).
const API_PORT = 18080;
const WEB_PORT = 3100;

export default defineConfig({
  testDir: "tests",
  timeout: 30_000,
  use: { baseURL: `http://127.0.0.1:${WEB_PORT}`, trace: "retain-on-failure" },
  webServer: [
    {
      command: `cargo build -q -p rcli-testkit --bin rcli-devstack && ../target/debug/rcli-devstack ${API_PORT} .devstack.json`,
      url: `http://127.0.0.1:${API_PORT}/api/v1/server-ca`,
      timeout: 600_000,
      reuseExistingServer: false,
      gracefulShutdown: { signal: "SIGTERM", timeout: 10_000 },
      stdout: "pipe",
    },
    {
      command: `pnpm build && pnpm start -p ${WEB_PORT}`,
      url: `http://127.0.0.1:${WEB_PORT}/signin`,
      timeout: 300_000,
      reuseExistingServer: false,
      env: { RCLI_API_URL: `http://127.0.0.1:${API_PORT}` },
    },
  ],
});
