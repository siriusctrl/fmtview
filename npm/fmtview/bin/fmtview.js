#!/usr/bin/env node

const { spawnSync } = require("node:child_process");
const { existsSync } = require("node:fs");
const { join } = require("node:path");

if (process.platform !== "linux" || process.arch !== "x64") {
  console.error(
    `fmtview npm package currently supports linux-x64 only, got ${process.platform}-${process.arch}.`,
  );
  process.exit(1);
}

const binary = join(__dirname, "..", "vendor", "fmtview");

if (!existsSync(binary)) {
  console.error(
    "fmtview binary is missing from this npm package. Reinstall fmtview or use cargo install.",
  );
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 1);
