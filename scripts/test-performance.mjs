import { spawn } from "node:child_process";

const memoryLimitBytes = Number.parseInt(
  process.env.FISHTANK_PERF_MEMORY_LIMIT_BYTES ?? `${4 * 1024 * 1024 * 1024}`,
  10,
);
const memoryLimitKib = Math.floor(memoryLimitBytes / 1024);
const binary = process.platform === "win32"
  ? "target/release/examples/performance.exe"
  : "target/release/examples/performance";

await run("cargo", ["test", "-p", "fishtank-core", "--test", "state_machine_stress"]);
await run("cargo", ["build", "--release", "-p", "fishtank-core", "--example", "performance"]);
await runWithMemoryLimit(binary, []);

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      stdio: "inherit",
      env: process.env,
    });
    child.on("error", reject);
    child.on("exit", (code, signal) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`${command} ${args.join(" ")} failed with ${signal ?? code}`));
      }
    });
  });
}

function runWithMemoryLimit(command, args) {
  console.log(`Running ${command} with RSS cap ${formatBytes(memoryLimitBytes)}`);
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      stdio: "inherit",
      env: process.env,
    });
    let killedForMemory = false;
    const monitor = setInterval(async () => {
      const rssKib = await readRssKib(child.pid);
      if (rssKib !== null && rssKib > memoryLimitKib) {
        killedForMemory = true;
        child.kill("SIGKILL");
      }
    }, 100);

    child.on("error", (error) => {
      clearInterval(monitor);
      reject(error);
    });
    child.on("exit", (code, signal) => {
      clearInterval(monitor);
      if (killedForMemory) {
        reject(new Error(`${command} exceeded RSS cap ${formatBytes(memoryLimitBytes)}`));
      } else if (code === 0) {
        resolve();
      } else {
        reject(new Error(`${command} failed with ${signal ?? code}`));
      }
    });
  });
}

function readRssKib(pid) {
  if (!pid) {
    return Promise.resolve(null);
  }
  return new Promise((resolve) => {
    const child = spawn("ps", ["-o", "rss=", "-p", String(pid)], {
      stdio: ["ignore", "pipe", "ignore"],
    });
    let stdout = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.on("error", () => resolve(null));
    child.on("exit", () => {
      const value = Number.parseInt(stdout.trim(), 10);
      resolve(Number.isFinite(value) ? value : null);
    });
  });
}

function formatBytes(bytes) {
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GiB`;
}
