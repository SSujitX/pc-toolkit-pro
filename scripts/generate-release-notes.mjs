#!/usr/bin/env node
/**
 * Categorize conventional commits between two refs for GitHub Release notes.
 *
 *   node ./scripts/generate-release-notes.mjs \
 *     --previous-tag v3.0.0 \
 *     --tag v3.0.1 \
 *     --bump patch \
 *     --from-version 3.0.0 \
 *     --version 3.0.1
 */
import { execSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const TYPE_ORDER = [
  "feat",
  "fix",
  "perf",
  "refactor",
  "docs",
  "test",
  "build",
  "ci",
  "chore",
  "revert",
  "other",
];

const TYPE_HEADINGS = {
  feat: "Features",
  fix: "Fixes",
  perf: "Performance",
  refactor: "Refactors",
  docs: "Docs",
  test: "Tests",
  build: "Build",
  ci: "CI",
  chore: "Chores",
  revert: "Reverts",
  other: "Other",
};

const SUBJECT_RE =
  /^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\([^)]+\))?(!)?:\s+/i;

export function classifySubject(subject) {
  const text = String(subject ?? "").trim();
  const match = SUBJECT_RE.exec(text);
  if (!match) {
    return { type: "other", breaking: false, subject: text };
  }
  const type = match[1].toLowerCase() === "style" ? "other" : match[1].toLowerCase();
  return {
    type: TYPE_HEADINGS[type] ? type : "other",
    breaking: match[3] === "!",
    subject: text,
  };
}

export function renderReleaseNotes({
  version,
  tag,
  bump,
  fromVersion,
  previousTag,
  repo,
  commits,
}) {
  const grouped = new Map(TYPE_ORDER.map((type) => [type, []]));
  const breaking = [];

  for (const commit of commits) {
    const classified = classifySubject(commit.subject);
    const line = `- ${classified.subject} (\`${commit.hash}\`)`;
    grouped.get(classified.type)?.push(line);
    if (classified.breaking) breaking.push(line);
  }

  const lines = [`## PC Toolkit Pro v${version}`, ""];
  if (bump && fromVersion && fromVersion !== version) {
    lines.push(`${bump} release (\`${fromVersion}\` → \`${version}\`).`, "");
  } else if (bump === "current") {
    lines.push(`Release of current VERSION \`${version}\`.`, "");
  }

  if (breaking.length) {
    lines.push("### Breaking changes", "", ...breaking, "");
  }

  for (const type of TYPE_ORDER) {
    const items = grouped.get(type) ?? [];
    if (!items.length) continue;
    lines.push(`### ${TYPE_HEADINGS[type]}`, "", ...items, "");
  }

  const total = commits.length;
  if (!total) {
    lines.push("_No commits in this range._", "");
  }

  if (repo) {
    const compare = previousTag
      ? `https://github.com/${repo}/compare/${encodeURIComponent(previousTag)}...${encodeURIComponent(tag)}`
      : `https://github.com/${repo}/releases/tag/${encodeURIComponent(tag)}`;
    lines.push(`**Full changelog:** ${compare}`, "");
  }

  lines.push(
    "### Downloads",
    "",
    `- Setup (current user, no admin): \`PC Toolkit Pro v${version} Setup.exe\``,
    `- Portable: \`PC Toolkit Pro v${version}.exe\``,
    ""
  );

  return lines.join("\n");
}

export function parseGitLog(raw) {
  const commits = [];
  for (const line of String(raw).split(/\r?\n/)) {
    if (!line.trim()) continue;
    const tab = line.indexOf("\t");
    if (tab === -1) continue;
    commits.push({
      hash: line.slice(0, tab).trim(),
      subject: line.slice(tab + 1).trim(),
    });
  }
  return commits;
}

function arg(name) {
  const flag = `--${name}`;
  const index = process.argv.indexOf(flag);
  if (index === -1) return "";
  const value = process.argv[index + 1] ?? "";
  if (!value || value.startsWith("--")) return "";
  return value;
}

function gitLog(previousTag) {
  const range = previousTag ? `${previousTag}..HEAD` : "HEAD";
  return execSync(`git log --no-merges --format=%h%x09%s ${range}`, {
    encoding: "utf8",
  });
}

function previousTagFallback() {
  try {
    return execSync("git describe --tags --abbrev=0", { encoding: "utf8" }).trim();
  } catch {
    return "";
  }
}

function main() {
  const version = arg("version");
  const tag = arg("tag") || (version ? `v${version}` : "");
  const bump = arg("bump") || "current";
  const fromVersion = arg("from-version");
  const previousTag = process.argv.includes("--previous-tag")
    ? arg("previous-tag")
    : previousTagFallback();
  const repo = process.env.GITHUB_REPOSITORY || "SSujitX/pc-toolkit-pro";

  if (!version) {
    throw new Error("Missing --version");
  }

  const commits = parseGitLog(gitLog(previousTag));
  const notes = renderReleaseNotes({
    version,
    tag,
    bump,
    fromVersion,
    previousTag,
    repo,
    commits,
  });
  process.stdout.write(notes);
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
