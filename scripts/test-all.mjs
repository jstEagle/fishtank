import { spawn } from "node:child_process";

const steps = [
  ["cargo", ["test", "--workspace"]],
  ["pnpm", ["--filter", "@fishtank/viewer", "typecheck"]],
  ["pnpm", ["--filter", "@fishtank/viewer", "lint"]],
  ["pnpm", ["--filter", "@fishtank/viewer", "test"]],
  ["pnpm", ["--filter", "@fishtank/viewer", "build"]],
  ["pnpm", ["--filter", "@fishtank/edge", "typecheck"]],
  ["pnpm", ["--filter", "@fishtank/edge", "test"]],
];

for (const [command, args] of steps) {
  await run(command, args);
}

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
