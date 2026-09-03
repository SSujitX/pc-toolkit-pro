import assert from "node:assert/strict";
import test from "node:test";
import {
  githubReleaseAssetName,
  updaterAssetUrl,
} from "./build-updater-latest.mjs";

test("githubReleaseAssetName matches GitHub space-to-dot storage", () => {
  assert.equal(
    githubReleaseAssetName("PC Toolkit Pro v3.0.1 Setup.exe"),
    "PC.Toolkit.Pro.v3.0.1.Setup.exe"
  );
  assert.equal(githubReleaseAssetName("latest.json"), "latest.json");
});

test("updaterAssetUrl uses the stored GitHub name not %20", () => {
  const url = updaterAssetUrl({
    owner: "SSujitX",
    repo: "pc-toolkit-pro",
    tag: "v3.0.1",
    fileName: "release-out/PC Toolkit Pro v3.0.1 Setup.exe",
  });
  assert.equal(
    url,
    "https://github.com/SSujitX/pc-toolkit-pro/releases/download/v3.0.1/PC.Toolkit.Pro.v3.0.1.Setup.exe"
  );
  assert.doesNotMatch(url, /%20/);
});
