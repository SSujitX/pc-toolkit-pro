#!/usr/bin/env node
/**
 * Sync product version from the root VERSION file into all package manifests.
 *
 * Usage:
 *   node ./scripts/sync-version.mjs          # sync
 *   node ./scripts/sync-version.mjs check
 *   node ./scripts/sync-version.mjs print
 *   pnpm version:sync | pnpm version:check
 *
 * Release (GitHub Actions):
 *   1. Bump VERSION (e.g. 3.0.1)
 *   2. pnpm version:sync
 *   3. Commit + tag v$(node ./scripts/sync-version.mjs print)
 *   4. Push tag → Release Windows workflow builds EXE and attaches artifacts
 */
import { execSync } from "node:child_process";
import fs from "node:fs";

const mode = process.argv[2] ?? "sync";

if (!fs.existsSync("VERSION")) {
  console.error("Missing VERSION file at repo root");
  process.exit(1);
}

const version = fs.readFileSync("VERSION", "utf8").trim();
if (!/^\d+\.\d+\.\d+$/.test(version)) {
  console.error(`Invalid VERSION (expected X.Y.Z): '${version}'`);
  process.exit(1);
}

if (mode === "print") {
  process.stdout.write(`${version}\n`);
  process.exit(0);
}

const files = {
  "package.json": "json",
  "src-tauri/tauri.conf.json": "json",
  "src-tauri/Cargo.toml": "cargo",
  "src-tauri/crates/pctoolkit-core/Cargo.toml": "cargo",
  "src-tauri/crates/pctoolkit-platform/Cargo.toml": "cargo",
  "src/lib/models/application-shell.ts": "ts",
};

function read(path, kind) {
  const text = fs.readFileSync(path, "utf8");
  if (kind === "json") return JSON.parse(text).version;
  if (kind === "cargo") {
    const m = text.match(/^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m);
    if (!m) throw new Error(`no package version in ${path}`);
    return m[1];
  }
  const m = text.match(/export\s+const\s+APP_VERSION\s*=\s*['"]([^'"]+)['"]/);
  if (!m) throw new Error(`no APP_VERSION in ${path}`);
  return m[1];
}

function write(path, kind) {
  if (kind === "json") {
    const data = JSON.parse(fs.readFileSync(path, "utf8"));
    data.version = version;
    fs.writeFileSync(path, JSON.stringify(data, null, 2) + "\n");
    return;
  }
  let text = fs.readFileSync(path, "utf8");
  let next;
  if (kind === "cargo") {
    next = text.replace(/^(\[package\][\s\S]*?^version\s*=\s*")[^"]+/m, `$1${version}`);
  } else {
    next = text.replace(
      /(export\s+const\s+APP_VERSION\s*=\s*['"])([^'"]+)(['"])/,
      `$1${version}$3`
    );
  }
  if (next === text) throw new Error(`Failed to update ${path}`);
  fs.writeFileSync(path, next);
}

if (mode === "check") {
  let ok = true;
  for (const [path, kind] of Object.entries(files)) {
    if (!fs.existsSync(path)) {
      console.error(`missing: ${path}`);
      ok = false;
      continue;
    }
    const got = read(path, kind);
    if (got !== version) {
      console.error(`${path} is ${got}, expected ${version}`);
      ok = false;
    }
  }
  if (!ok) process.exit(1);
  console.log(`OK: all package files match VERSION ${version}`);
  process.exit(0);
}

if (mode !== "sync") {
  console.error("Usage: node ./scripts/sync-version.mjs [sync|check|print]");
  process.exit(1);
}

console.log(`Syncing product version ${version}`);
for (const [path, kind] of Object.entries(files)) write(path, kind);

try {
  execSync(`pnpm version ${version} --no-git-tag-version --allow-same-version`, {
    stdio: "ignore",
  });
} catch {
  // pnpm optional for environments that only need file sync
}

console.log(`Done: package files now at ${version}`);
execSync("node ./scripts/sync-version.mjs check", { stdio: "inherit" });
