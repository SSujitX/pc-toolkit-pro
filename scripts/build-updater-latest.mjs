#!/usr/bin/env node
/**
 * Builds Tauri updater latest.json for GitHub Releases.
 *
 * GitHub stores release asset names with spaces as periods
 * (`PC Toolkit Pro v3.0.1 Setup.exe` → `PC.Toolkit.Pro.v3.0.1.Setup.exe`).
 * latest.json must use that stored name or the updater gets 404.
 *
 * Usage:
 *   node ./scripts/build-updater-latest.mjs \
 *     --version 3.0.1 \
 *     --notes "..." \
 *     --setup path/to/setup.exe \
 *     --signature path/to/setup.exe.sig \
 *     --out release-out/latest.json
 */
import { readFileSync, writeFileSync } from "node:fs";
import { basename } from "node:path";
import path from "node:path";
import { fileURLToPath } from "node:url";

export function githubReleaseAssetName(fileName) {
  return String(fileName ?? "").replace(/ /g, ".");
}

export function updaterAssetUrl({ owner, repo, tag, fileName }) {
  const asset = githubReleaseAssetName(basename(fileName));
  return `https://github.com/${owner}/${repo}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(asset)}`;
}

function arg(name) {
  const index = process.argv.indexOf(`--${name}`);
  if (index === -1 || !process.argv[index + 1]) {
    throw new Error(`Missing --${name}`);
  }
  return process.argv[index + 1];
}

function main() {
  const version = arg("version");
  const notes = process.argv.includes("--notes") ? arg("notes") : "";
  const setupPath = arg("setup");
  const signaturePath = arg("signature");
  const outPath = arg("out");
  const owner = process.env.GITHUB_REPOSITORY_OWNER || "SSujitX";
  const repo = (process.env.GITHUB_REPOSITORY || "SSujitX/pc-toolkit-pro").split("/")[1];
  const tag = process.env.RELEASE_TAG || `v${version}`;

  const signature = readFileSync(signaturePath, "utf8").trim();
  const url = updaterAssetUrl({ owner, repo, tag, fileName: setupPath });

  const payload = {
    version,
    notes,
    pub_date: new Date().toISOString(),
    platforms: {
      "windows-x86_64": {
        signature,
        url,
      },
    },
  };

  writeFileSync(outPath, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
  console.log(`Wrote ${outPath}`);
  console.log(`  version=${version}`);
  console.log(`  url=${url}`);
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
