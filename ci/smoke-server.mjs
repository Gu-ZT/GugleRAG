import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";

const executable = process.argv[2];
if (!executable) throw new Error("usage: node ci/smoke-server.mjs <server-executable>");

fs.mkdirSync("ci-output", { recursive: true });
const log = fs.openSync("ci-output/server.log", "w");
const port = 18080;
const child = spawn(path.resolve(executable), [], {
  cwd: process.cwd(),
  env: {
    ...process.env,
    SERVER_HOST: "127.0.0.1",
    SERVER_PORT: String(port),
    DATABASE_URL: "sqlite://ci-output/smoke.db?mode=rwc",
    JWT_SECRET: "ci-smoke-test-secret",
    MCP_ENABLED: "true",
    MCP_AUTH_REQUIRED: "true"
  },
  stdio: ["ignore", log, log]
});

async function waitForHealth() {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`server exited with code ${child.exitCode}`);
    try {
      const response = await fetch(`http://127.0.0.1:${port}/health`);
      if (response.ok) return response;
    } catch {
      // The server may still be starting.
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error("server did not become healthy within 30 seconds");
}

try {
  const health = await waitForHealth();
  const body = await health.text();
  if (!body.toLowerCase().includes("ok")) throw new Error(`unexpected health body: ${body}`);
  const setup = await fetch(`http://127.0.0.1:${port}/api/setup/status`);
  if (!setup.ok) throw new Error(`setup status returned ${setup.status}`);
  const page = await fetch(`http://127.0.0.1:${port}/`);
  if (!page.ok || !(await page.text()).includes('<div id="app"></div>')) {
    throw new Error("frontend index was not served by the backend");
  }
  console.log("server health, setup API, and static frontend smoke checks passed");
} finally {
  child.kill("SIGTERM");
  await new Promise((resolve) => {
    const timeout = setTimeout(resolve, 5_000);
    child.once("exit", () => {
      clearTimeout(timeout);
      resolve();
    });
  });
  fs.closeSync(log);
}
