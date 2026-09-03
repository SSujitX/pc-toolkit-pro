import assert from "node:assert/strict";
import test from "node:test";
import { nextVersion, parseSemver } from "./sync-version.mjs";

test("parseSemver accepts X.Y.Z", () => {
  assert.deepEqual(parseSemver("3.0.0"), {
    major: 3,
    minor: 0,
    patch: 0,
    text: "3.0.0",
  });
});

test("parseSemver rejects junk", () => {
  assert.throws(() => parseSemver("v3.0.0"), /expected X\.Y\.Z/);
  assert.throws(() => parseSemver("3.0"), /expected X\.Y\.Z/);
});

test("nextVersion bump kinds", () => {
  assert.equal(nextVersion("1.2.3", "current"), "1.2.3");
  assert.equal(nextVersion("1.2.3", "patch"), "1.2.4");
  assert.equal(nextVersion("1.2.3", "minor"), "1.3.0");
  assert.equal(nextVersion("1.2.3", "major"), "2.0.0");
});
