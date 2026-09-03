import assert from "node:assert/strict";
import test from "node:test";
import {
  classifySubject,
  parseGitLog,
  renderReleaseNotes,
} from "./generate-release-notes.mjs";

test("classifySubject maps conventional types", () => {
  assert.equal(classifySubject("feat(cleaner): add scan").type, "feat");
  assert.equal(classifySubject("fix(deep-cleaner): skip junctions").type, "fix");
  assert.equal(classifySubject("docs(agents): note recycle").type, "docs");
  assert.equal(classifySubject("WIP junk").type, "other");
  assert.equal(classifySubject("feat!: break the api").breaking, true);
});

test("parseGitLog reads hash + subject", () => {
  const commits = parseGitLog("abc1234\tfix(ui): jitter\n");
  assert.deepEqual(commits, [{ hash: "abc1234", subject: "fix(ui): jitter" }]);
});

test("renderReleaseNotes groups and lists downloads", () => {
  const md = renderReleaseNotes({
    version: "3.0.1",
    tag: "v3.0.1",
    bump: "patch",
    fromVersion: "3.0.0",
    previousTag: "v3.0.0",
    repo: "SSujitX/pc-toolkit-pro",
    commits: [
      { hash: "aaa1111", subject: "feat(memory): live stats" },
      { hash: "bbb2222", subject: "fix(cleaner): honest recycle size" },
      { hash: "ccc3333", subject: "random note" },
    ],
  });
  assert.match(md, /## PC Toolkit Pro v3\.0\.1/);
  assert.match(md, /### Features/);
  assert.match(md, /### Fixes/);
  assert.match(md, /### Other/);
  assert.match(md, /PC Toolkit Pro v3\.0\.1 Setup\.exe/);
  assert.match(md, /PC Toolkit Pro v3\.0\.1\.exe/);
  assert.match(md, /compare\/v3\.0\.0\.\.\.v3\.0\.1/);
});
