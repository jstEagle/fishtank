#!/usr/bin/env node
import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import process from "node:process";

const ROOT = new URL("..", import.meta.url).pathname;
const VIEWER_LOCK = new URL("../apps/viewer/.next/dev/lock", import.meta.url).pathname;
const HOST = process.env.FISHTANK_DEV_HOST ?? "127.0.0.1";
const API_START_PORT = intEnv("FISHTANK_API_PORT", 3838);
const VIEWER_START_PORT = intEnv("FISHTANK_VIEWER_PORT", 3000);
const WORLD_PATH = process.env.FISHTANK_WORLD ?? "worlds/village.json";
const STATE_PATH = process.env.FISHTANK_STATE ?? ".fishtank/dev";
const MAX_LOG_LINES = intEnv("FISHTANK_DEV_LOG_LINES", 14);
const HEALTH_POLL_MS = 750;
const CLEAR = "\x1b[2J\x1b[H";
const HIDE_CURSOR = "\x1b[?25l";
const SHOW_CURSOR = "\x1b[?25h";
const OSC8_START = "\x1b]8;;";
const OSC8_END = "\x1b\\";

const services = {
  server: service("Rust server"),
  viewer: service("Next viewer")
};

let apiPort = null;
let viewerPort = null;
let apiUrl = null;
let viewerUrl = null;
let stopping = false;
let renderTimer = null;
let healthTimer = null;

main().catch((error) => {
  console.error(error);
  process.exit(1);
});

async function main() {
  const existingApiUrl = `http://${HOST}:${API_START_PORT}`;
  const existingServerReady = await httpOk(`${existingApiUrl}/health`);
  if (existingServerReady) {
    apiPort = API_START_PORT;
    apiUrl = existingApiUrl;
    services.server.status = "reusing";
    services.server.ready = true;
    services.server.url = apiUrl;
    services.server.external = true;
    addLog("server", `reusing existing server at ${apiUrl}`);
  } else {
    apiPort = await findOpenPort(API_START_PORT, HOST);
    apiUrl = `http://${HOST}:${apiPort}`;
  }

  const existingViewer = await readExistingViewer();
  if (existingViewer) {
    viewerUrl = existingViewer.appUrl;
    viewerPort = existingViewer.port;
    services.viewer.status = "reusing";
    services.viewer.ready = true;
    services.viewer.pid = existingViewer.pid;
    services.viewer.url = viewerUrl;
    services.viewer.external = true;
    addLog("viewer", `reusing existing Next viewer at ${viewerUrl}`);
  } else {
    viewerPort = await findOpenPort(VIEWER_START_PORT, HOST, new Set([apiPort]));
    viewerUrl = `http://localhost:${viewerPort}`;
  }

  process.stdout.write(HIDE_CURSOR);
  process.stdout.on("resize", render);

  if (!services.server.external) {
    startServer();
  }
  if (!services.viewer.external) {
    startViewer();
  }
  healthTimer = setInterval(checkHealth, HEALTH_POLL_MS);
  renderTimer = setInterval(render, 250);
  render();

  for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
    process.on(signal, () => void shutdown(signal));
  }

  process.on("exit", () => {
    process.stdout.write(SHOW_CURSOR);
  });
}

function startServer() {
  services.server.status = "starting";
  services.server.command = `cargo run -p fishtank-server -- serve --world ${WORLD_PATH} --state ${STATE_PATH} --bind ${HOST}:${apiPort}`;
  services.server.child = spawn(
    "cargo",
    [
      "run",
      "-p",
      "fishtank-server",
      "--",
      "serve",
      "--world",
      WORLD_PATH,
      "--state",
      STATE_PATH,
      "--bind",
      `${HOST}:${apiPort}`
    ],
    {
      cwd: ROOT,
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        ...process.env,
        CARGO_TERM_COLOR: process.env.CARGO_TERM_COLOR ?? "always"
      }
    }
  );
  wireChild("server", services.server.child);
}

function startViewer() {
  services.viewer.status = "starting";
  services.viewer.command = `pnpm --filter @fishtank/viewer exec next dev --hostname ${HOST} --port ${viewerPort}`;
  services.viewer.child = spawn(
    "pnpm",
    ["--filter", "@fishtank/viewer", "exec", "next", "dev", "--hostname", HOST, "--port", String(viewerPort)],
    {
      cwd: ROOT,
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        ...process.env,
        NEXT_PUBLIC_FISHTANK_API_URL: apiUrl,
        BROWSER: "none"
      }
    }
  );
  wireChild("viewer", services.viewer.child);
}

function wireChild(name, child) {
  const target = services[name];
  target.pid = child.pid ?? null;
  collectOutput(name, child.stdout);
  collectOutput(name, child.stderr);

  child.on("error", (error) => {
    target.status = "failed";
    target.error = error.message;
    addLog(name, `process error: ${error.message}`);
    render();
  });

  child.on("exit", (code, signal) => {
    target.pid = null;
    target.ready = false;
    target.exit = signal ? `signal ${signal}` : `code ${code}`;
    if (stopping) {
      target.status = "stopped";
    } else {
      target.status = code === 0 ? "stopped" : "failed";
      target.error = target.exit;
      addLog(name, `exited with ${target.exit}`);
      void shutdown(`${name} exited`);
    }
    render();
  });
}

function collectOutput(name, stream) {
  stream.setEncoding("utf8");
  let pending = "";
  stream.on("data", (chunk) => {
    pending += stripAnsi(chunk);
    const lines = pending.split(/\r?\n/);
    pending = lines.pop() ?? "";
    for (const line of lines) {
      if (line.trim()) {
        addLog(name, line);
        updateStatusFromLog(name, line);
      }
    }
    render();
  });
}

function updateStatusFromLog(name, line) {
  if (name === "server" && line.includes("Running `")) {
    services.server.status = "binding";
  }
  if (name === "viewer") {
    const localMatch = line.match(/Local:\s+(https?:\/\/\S+)/);
    if (localMatch) {
      viewerUrl = localMatch[1];
      services.viewer.url = viewerUrl;
    }
    if (line.includes("Ready in") || line.includes("Compiled")) {
      services.viewer.status = "ready";
    }
  }
}

async function checkHealth() {
  services.server.ready = await httpOk(`${apiUrl}/health`);
  services.server.url = apiUrl;
  if (services.server.ready && !["failed", "stopped"].includes(services.server.status)) {
    services.server.status = "ready";
  }

  services.viewer.ready = await tcpOkFromUrl(viewerUrl);
  services.viewer.url = viewerUrl;
  if (services.viewer.ready && !["failed", "stopped"].includes(services.viewer.status)) {
    services.viewer.status = "ready";
  }
  render();
}

async function shutdown(reason) {
  if (stopping) {
    return;
  }
  stopping = true;
  clearInterval(renderTimer);
  clearInterval(healthTimer);

  addLog("dev", `shutting down: ${reason}`);
  render();

  const children = [services.viewer.child, services.server.child].filter((child) => child && child.pid);
  for (const entry of [services.viewer, services.server]) {
    if (entry.child?.pid) {
      entry.status = "stopping";
      entry.ready = false;
    }
  }
  for (const child of children) {
    child.kill("SIGTERM");
  }

  await Promise.race([
    Promise.all(children.map(waitForExit)),
    delay(2500)
  ]);

  for (const child of children) {
    if (child.pid && child.exitCode == null && child.signalCode == null) {
      child.kill("SIGKILL");
    }
  }

  render();
  process.stdout.write(SHOW_CURSOR);
  process.exit(reason === "SIGINT" || reason === "SIGTERM" || reason === "SIGHUP" ? 0 : 1);
}

function render() {
  const width = process.stdout.columns || 100;
  const contentWidth = Math.max(72, width);
  const logWidth = Math.max(30, contentWidth - 18);
  const rows = [];
  rows.push(bold("Fishtank dev"));
  rows.push(dim("One command local stack. Press Ctrl-C to stop both processes."));
  rows.push("");
  rows.push(section("Services", contentWidth));
  rows.push(tableHeader(contentWidth));
  rows.push(statusLine("Rust server", services.server, apiUrl, contentWidth));
  rows.push(statusLine("Next viewer", services.viewer, viewerUrl, contentWidth));
  rows.push("");
  rows.push(section("Links", contentWidth));
  rows.push(linkLine("Open viewer", viewerUrl));
  rows.push(linkLine("Rust API", apiUrl));
  rows.push(dim("Tip: the full URLs are visible for copy, and OSC-8 wrapped for Command-click in supported terminals."));
  rows.push("");
  rows.push(section("Recent Logs", contentWidth));
  const recentLogs = logs().slice(-MAX_LOG_LINES);
  if (recentLogs.length === 0) {
    rows.push(dim("No logs yet."));
  } else {
    rows.push(...recentLogs.map((line) => formatLogLine(line, logWidth)));
  }
  process.stdout.write(`${CLEAR}${rows.join("\n")}\n`);
}

function tableHeader(width) {
  return [
    padVisible(dim("Service"), 14),
    padVisible(dim("Status"), 12),
    padVisible(dim("PID"), 10),
    dim("URL")
  ].join(" ");
}

function statusLine(label, data, url) {
  const status = statusBadge(data);
  const pid = data.pid ? dim(`pid ${data.pid}`) : dim("no pid");
  const suffix = data.error ? red(` ${data.error}`) : "";
  return [
    padVisible(label, 14),
    padVisible(status, 12),
    padVisible(pid, 10),
    hyperlink(url, url),
    suffix
  ].join(" ");
}

function statusBadge(data) {
  if (data.status === "failed") {
    return red("FAILED");
  }
  if (data.status === "stopped") {
    return yellow("STOPPED");
  }
  if (data.status === "stopping") {
    return yellow("STOPPING");
  }
  if (data.ready) {
    return green(data.external ? "REUSED" : "READY");
  }
  return yellow(data.status.toUpperCase());
}

function section(label, width) {
  const left = `-- ${label} `;
  return `${dim(left)}${dim("-".repeat(Math.max(0, width - visibleLength(left))))}`;
}

function linkLine(label, url) {
  return `${padVisible(label, 14)} ${hyperlink(url, url)}`;
}

function formatLogLine(line, width) {
  const match = line.match(/^\[([^\]]+)]\s+(\S+)\s+(.*)$/);
  if (!match) {
    return truncateVisible(line, width + 18);
  }
  const [, time, source, message] = match;
  const sourceColor = source === "server" ? cyan(source) : source === "viewer" ? magenta(source) : dim(source);
  return [
    dim(time.padStart(11)),
    padVisible(sourceColor, 8),
    truncateVisible(message, width)
  ].join(" ");
}

function addLog(name, line) {
  const target = services[name] ?? service(name);
  const timestamp = new Date().toLocaleTimeString();
  target.logs.push(`[${timestamp}] ${name.padEnd(6)} ${line}`);
  if (target.logs.length > MAX_LOG_LINES * 3) {
    target.logs.splice(0, target.logs.length - MAX_LOG_LINES * 3);
  }
  if (!services[name]) {
    services[name] = target;
  }
}

function logs() {
  return Object.values(services).flatMap((entry) => entry.logs);
}

function service(label) {
  return {
    label,
    status: "pending",
    ready: false,
    pid: null,
    url: null,
    child: null,
    logs: [],
    error: null,
    command: null,
    exit: null,
    external: false
  };
}

async function readExistingViewer() {
  try {
    const raw = await fs.readFile(VIEWER_LOCK, "utf8");
    const parsed = JSON.parse(raw);
    if (
      Number.isInteger(parsed.pid) &&
      Number.isInteger(parsed.port) &&
      typeof parsed.appUrl === "string" &&
      processExists(parsed.pid) &&
      await tcpOkFromUrl(parsed.appUrl)
    ) {
      return parsed;
    }
    addLog("viewer", "removing stale Next dev lock");
    await fs.rm(VIEWER_LOCK, { force: true });
  } catch (error) {
    if (error?.code !== "ENOENT") {
      addLog("viewer", `could not read Next dev lock: ${error.message}`);
    }
  }
  return null;
}

function processExists(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function findOpenPort(start, host, reserved = new Set()) {
  const attempts = 80;
  return new Promise((resolve, reject) => {
    let port = start;

    const tryNext = () => {
      if (reserved.has(port)) {
        port += 1;
        tryNext();
        return;
      }

      if (port >= start + attempts) {
        reject(new Error(`no open port found from ${start} to ${start + attempts - 1}`));
        return;
      }

      const server = net.createServer();
      server.unref();
      server.on("error", () => {
        port += 1;
        tryNext();
      });
      server.listen({ host, port }, () => {
        const selected = server.address().port;
        server.close(() => resolve(selected));
      });
    };

    tryNext();
  });
}

function httpOk(url) {
  return new Promise((resolve) => {
    const request = http.get(url, { timeout: 700 }, (response) => {
      response.resume();
      resolve(response.statusCode >= 200 && response.statusCode < 500);
    });
    request.on("timeout", () => {
      request.destroy();
      resolve(false);
    });
    request.on("error", () => resolve(false));
  });
}

function tcpOkFromUrl(url) {
  try {
    const parsed = new URL(url);
    return tcpOk(parsed.hostname, Number(parsed.port || 80));
  } catch {
    return Promise.resolve(false);
  }
}

function tcpOk(host, port) {
  return new Promise((resolve) => {
    const socket = net.createConnection({ host, port });
    const done = (ok) => {
      socket.removeAllListeners();
      socket.destroy();
      resolve(ok);
    };
    socket.setTimeout(700);
    socket.once("connect", () => done(true));
    socket.once("timeout", () => done(false));
    socket.once("error", () => done(false));
  });
}

function waitForExit(child) {
  return new Promise((resolve) => {
    if (child.exitCode != null || child.signalCode != null) {
      resolve();
      return;
    }
    child.once("exit", resolve);
  });
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function intEnv(name, fallback) {
  const value = Number.parseInt(process.env[name] ?? "", 10);
  return Number.isFinite(value) ? value : fallback;
}

function truncateVisible(input, width) {
  if (visibleLength(input) <= width) {
    return input;
  }

  let output = "";
  let visible = 0;
  for (let index = 0; index < input.length && visible < width - 1;) {
    if (input.startsWith("\x1b]8;;", index)) {
      const end = input.indexOf(OSC8_END, index);
      if (end === -1) {
        break;
      }
      output += input.slice(index, end + OSC8_END.length);
      index = end + OSC8_END.length;
      continue;
    }
    if (input[index] === "\x1b") {
      const match = input.slice(index).match(/^\x1b\[[0-9;?]*[ -/]*[@-~]/);
      if (match) {
        output += match[0];
        index += match[0].length;
        continue;
      }
    }
    output += input[index];
    visible += 1;
    index += 1;
  }
  return `${output}...`;
}

function padVisible(input, width) {
  const visible = visibleLength(input);
  return visible >= width ? input : `${input}${" ".repeat(width - visible)}`;
}

function visibleLength(input) {
  return stripAnsi(stripOsc8(input)).length;
}

function hyperlink(url, label) {
  if (!supportsHyperlinks()) {
    return label;
  }
  return `${OSC8_START}${url}${OSC8_END}${label}${OSC8_START}${OSC8_END}`;
}

function supportsHyperlinks() {
  return process.env.FISHTANK_NO_LINKS !== "1" && process.stdout.isTTY && process.env.TERM !== "dumb";
}

function stripOsc8(input) {
  return input.replace(/\x1b]8;;.*?\x1b\\/g, "");
}

function stripAnsi(input) {
  return input.replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, "");
}

function bold(input) {
  return `\x1b[1m${input}\x1b[22m`;
}

function dim(input) {
  return `\x1b[2m${input}\x1b[22m`;
}

function green(input) {
  return `\x1b[32m${input}\x1b[39m`;
}

function cyan(input) {
  return `\x1b[36m${input}\x1b[39m`;
}

function magenta(input) {
  return `\x1b[35m${input}\x1b[39m`;
}

function yellow(input) {
  return `\x1b[33m${input}\x1b[39m`;
}

function red(input) {
  return `\x1b[31m${input}\x1b[39m`;
}
