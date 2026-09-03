#!/usr/bin/env node
/**
 * Sync product version from the root VERSION file into all package manifests.
 *
 * Usage:
 *   node ./scripts/sync-version.mjs              # sync from VERSION
 *   node ./scripts/sync-version.mjs check
 *   node ./scripts/sync-version.mjs print
 *   node ./scripts/sync-version.mjs bump patch   # current | patch | minor | major
 *   pnpm version:sync | pnpm version:check
 *
 * Release: Actions → Release → bump. Do not free-type semver.
 */
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const VERSION_RE = /^(\d+)\.(\d+)\.(\d+)$/;
const LOCK_PACKAGES = ["pc-toolkit-pro", "pctoolkit-core", "pctoolkit-platform"];

function lockVersionRe(name, capture = true) {
  const version = capture ? '"([^"]+)"' : '"[^"]+"';
  return new RegExp(`(name = "${name}"\\r?\\nversion = )${version}`);
}

export function parseLockPackageVersion(text, name) {
  const m = String(text).match(lockVersionRe(name, true));
  return m ? m[2] : null;
}

const MANIFESTS = {
  "package.json": "json",
  "src-tauri/tauri.conf.json": "json",
  "src-tauri/Cargo.toml": "cargo",
  "src-tauri/crates/pctoolkit-core/Cargo.toml": "cargo",
  "src-tauri/crates/pctoolkit-platform/Cargo.toml": "cargo",
  "src/lib/models/application-shell.ts": "ts",
};

export function parseSemver(value) {
  const trimmed = String(value ?? "").trim();
  const match = VERSION_RE.exec(trimmed);
  if (!match) {
    throw new Error(`Invalid VERSION (expected X.Y.Z): '${trimmed}'`);
  }
  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
    text: trimmed,
  };
}

export function nextVersion(current, bump) {
  const parsed = parseSemver(current);
  switch (bump) {
    case "current":
      return parsed.text;
    case "patch":
      return `${parsed.major}.${parsed.minor}.${parsed.patch + 1}`;
    case "minor":
      return `${parsed.major}.${parsed.minor + 1}.0`;
    case "major":
      return `${parsed.major + 1}.0.0`;
    default:
      throw new Error(`unknown bump '${bump}' (use current|patch|minor|major)`);
  }
}

function readVersionFile() {
  if (!fs.existsSync("VERSION")) {
    throw new Error("Missing VERSION file at repo root");
  }
  return parseSemver(fs.readFileSync("VERSION", "utf8")).text;
}

function writeVersionFile(version) {
  fs.writeFileSync("VERSION", `${version}\n`);
}

function readManifest(path, kind) {
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

function writeManifest(path, kind, version) {
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

function writeCargoLock(path, version) {
  let text = fs.readFileSync(path, "utf8");
  for (const name of LOCK_PACKAGES) {
    const re = new RegExp(`(name = "${name}"\\r?\\nversion = ")[^"]+`, "g");
    const next = text.replace(re, `$1${version}`);
    if (next === text) throw new Error(`Failed to update ${name} in ${path}`);
    text = next;
  }
  fs.writeFileSync(path, text);
}

function readCargoLock(path) {
  const text = fs.readFileSync(path, "utf8");
  const found = {};
  for (const name of LOCK_PACKAGES) {
    const got = parseLockPackageVersion(text, name);
    if (!got) throw new Error(`no ${name} version in ${path}`);
    found[name] = got;
  }
  return found;
}

export function syncFromVersion(version) {
  for (const [path, kind] of Object.entries(MANIFESTS)) {
    writeManifest(path, kind, version);
  }
  if (fs.existsSync("src-tauri/Cargo.lock")) {
    writeCargoLock("src-tauri/Cargo.lock", version);
  }
  try {
    execSync(`pnpm version ${version} --no-git-tag-version --allow-same-version`, {
      stdio: "ignore",
    });
  } catch {
    // pnpm optional for environments that only need file sync
  }
}

export function checkVersion(version) {
  const errors = [];
  for (const [path, kind] of Object.entries(MANIFESTS)) {
    if (!fs.existsSync(path)) {
      errors.push(`missing: ${path}`);
      continue;
    }
    const got = readManifest(path, kind);
    if (got !== version) errors.push(`${path} is ${got}, expected ${version}`);
  }
  if (fs.existsSync("src-tauri/Cargo.lock")) {
    const lock = readCargoLock("src-tauri/Cargo.lock");
    for (const [name, got] of Object.entries(lock)) {
      if (got !== version) {
        errors.push(`src-tauri/Cargo.lock ${name} is ${got}, expected ${version}`);
      }
    }
  }
  return errors;
}

function main() {
  const mode = process.argv[2] ?? "sync";
  const bumpKind = process.argv[3];

  if (mode === "print") {
    process.stdout.write(`${readVersionFile()}\n`);
    return;
  }

  if (mode === "bump") {
    const current = readVersionFile();
    const version = nextVersion(current, bumpKind);
    writeVersionFile(version);
    console.log(`bump ${bumpKind}: ${current} -> ${version}`);
    syncFromVersion(version);
    const errors = checkVersion(version);
    if (errors.length) {
      for (const line of errors) console.error(line);
      process.exit(1);
    }
    console.log(`Done: package files now at ${version}`);
    return;
  }

  const version = readVersionFile();

  if (mode === "check") {
    const errors = checkVersion(version);
    if (errors.length) {
      for (const line of errors) console.error(line);
      process.exit(1);
    }
    console.log(`OK: all package files match VERSION ${version}`);
    return;
  }

  if (mode !== "sync") {
    console.error("Usage: node ./scripts/sync-version.mjs [sync|check|print|bump <kind>]");
    process.exit(1);
  }

  console.log(`Syncing product version ${version}`);
  syncFromVersion(version);
  const errors = checkVersion(version);
  if (errors.length) {
    for (const line of errors) console.error(line);
    process.exit(1);
  }
  console.log(`Done: package files now at ${version}`);
}

const isMain = (() => {
  try {
    return (
      path.resolve(fileURLToPath(import.meta.url)) === path.resolve(process.argv[1] ?? "")
    );
  } catch {
    return false;
  }
})();
if (isMain) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
}
