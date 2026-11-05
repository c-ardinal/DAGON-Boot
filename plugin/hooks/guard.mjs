// Guard hook for DAGON port sessions.
//
//   node guard.mjs        PreToolUse: deny writes before they happen
//   node guard.mjs post   PostToolUse (shell): detect writes that slipped through
//
// During a port session the conformance suite, the simulator and the core
// state machine must stay exactly as reviewed. A port session is any of:
//   - a call made by one of this plugin's agents (agent_type "dagon:*"),
//   - DAGON_PORT_SESSION=1 in the environment (Agent SDK runs),
//   - the repository of the session cwd (or of the written file) has a
//     worktree on a "port/*" branch (the dagon-port skill runs each attempt
//     in such a worktree).
// Additionally port-engineer may only write in the ESP32-S3 port area (the
// MCU package targets/mcu/esp32s3/, board packages under targets/boards/ that
// build on it or are new, samples/esp32s3-*/ and targets/esp32s3-*/) and
// conformance-judge may only run `dagon conform` / `dagon sim`.
//
// shell commands are judged by string inspection (fail-closed on
// substitutions); globs, patch tools and build scripts are caught afterwards
// by the PostToolUse diff check, not prevented.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const PROTECTED = ["crates/dagon-conform", "crates/dagon-sim", "crates/dagon-core"];
const PROTECTED_IN_PATH = /(^|\/)crates\/dagon-(conform|sim|core)(\/|$)/i;
const PROTECTED_WORD = /dagon-(conform|sim|core)\b/i;
const PACKAGE_FLAG = /(^|\s)(-p|--package)(\s+|=)\S+/g;
// Port area of port-engineer, relative to the repository root.
const ENGINEER_AREAS = [/^targets\/esp32s3-[^/]+\//i, /^targets\/mcu\/esp32s3\//i, /^samples\/esp32s3-[^/]+\//i];
// A board package is in the area when it builds on the ESP32-S3 MCU package,
// or does not exist yet (a new board).
const BOARD_DIR = /^targets\/boards\/([^/]+)\//i;
const ESP_MCU_PACKAGE = "dagon-mcu-esp32s3";

// Whether a Cargo.toml lists `pkg` as a dependency: a `pkg = ...` key in a
// `[dependencies]` table (also `[target.'...'.dependencies]`), or a
// `[dependencies.pkg]` table. Comments and other tables (dev-dependencies,
// features, metadata) do not count.
export function dependsOn(manifest, pkg) {
  let inDeps = false;
  for (const raw of String(manifest).split(/\r?\n/)) {
    const line = raw.replace(/#.*$/, "").trim();
    const table = /^\[([^[\]]+)\]$/.exec(line);
    if (table) {
      const name = table[1].trim();
      if (name === `dependencies.${pkg}` || name.endsWith(`.dependencies.${pkg}`)) return true;
      inDeps = name === "dependencies" || name.endsWith(".dependencies");
      continue;
    }
    const key = /^["']?([A-Za-z0-9_-]+)["']?\s*=/.exec(line);
    if (inDeps && key?.[1] === pkg) return true;
  }
  return false;
}
const ENGINEER_AREA_TEXT = "targets/mcu/esp32s3/, ESP32-S3 board packages under targets/boards/, samples/esp32s3-*/ or targets/esp32s3-*/";
const SUBSTITUTION = /\$\(|`|<\(|>\(/;
const JUDGE_COMMAND = /^[ \t]*dagon(\.exe)?[ \t]+(conform|sim)([ \t][^&|;$`<>(){}\r\n]*)?$/i;
const FILE_TOOLS = new Set(["Write", "Edit", "MultiEdit", "NotebookEdit"]);
const SHELL_TOOLS = new Set(["Bash", "PowerShell"]);
const ENGINEER = "dagon:port-engineer";
const JUDGE = "dagon:conformance-judge";
const CD_COMMANDS = new Set(["cd", "pushd", "popd", "chdir", "set-location", "sl"]);
const NULL_SINKS = new Set(["/dev/null", "$null", "nul"]);

const READONLY_COMMANDS = new Set([
  "cat", "head", "tail", "less", "grep", "rg", "ls", "dir", "wc", "diff", "pwd", "echo",
  "get-content", "select-string", "get-childitem", "test-path", ...CD_COMMANDS,
]);
const READONLY_SUBCOMMANDS = {
  git: new Set(["diff", "log", "show", "status", "blame", "grep", "ls-files", "rev-parse"]),
  cargo: new Set(["build", "check", "test", "clippy", "doc", "tree", "metadata"]),
  dagon: null, // every dagon subcommand only reads the source tree
};
const FIND_WRITES = /^-(delete|exec|execdir|ok|okdir|fprint|fprint0|fprintf|fls)$/i;

const MSG_PROTECTED =
  "Port session: crates/dagon-conform, crates/dagon-sim and crates/dagon-core are read-only. " +
  "Fix the target, not the tests. If a test looks wrong, stop and report to a human.";
const MSG_SUBSTITUTION =
  "Port session: command substitution ($(...), backticks, <(...)) is not allowed; run the inner command on its own.";
const MSG_ENGINEER =
  `port-engineer may only edit files under ${ENGINEER_AREA_TEXT} (shell writes: cd into the target directory first, ` +
  "no absolute, '..', '~' or '$' arguments). Changes to shared crates go to a human, who forwards them to the firmware owners.";
const MSG_JUDGE_WRITE = "conformance-judge has no write access.";
const MSG_JUDGE_SHELL =
  "conformance-judge may only run a single `dagon conform ...` or `dagon sim ...` command without shell operators.";

const slash = (p) => String(p ?? "").replace(/\\/g, "/");
const absPath = (cwd, p) => slash(resolve(cwd ?? ".", p));
const unquote = (w) => w.replace(/^["']+|["']+$/g, "");

// ---------- git access (replaced by stubs in tests) ----------

function git(dir, args) {
  let d = dir;
  while (d && !(existsSync(d) && statSync(d).isDirectory())) {
    const up = dirname(d);
    if (up === d) return null;
    d = up;
  }
  try {
    return execFileSync("git", ["-C", d, ...args], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      timeout: 5000,
    }); // untrimmed: porcelain output starts with significant spaces
  } catch {
    return null;
  }
}

function parsePortWorktrees(porcelain) {
  return String(porcelain ?? "")
    .split(/\r?\n\r?\n/)
    .map((block) => ({
      path: /^worktree (.+)$/m.exec(block)?.[1],
      branch: /^branch refs\/heads\/(.+)$/m.exec(block)?.[1] ?? "",
    }))
    .filter((w) => w.path && w.branch.startsWith("port/"))
    .map((w) => w.path);
}

export const realDeps = {
  env: process.env,
  readText: (file) => {
    try {
      return readFileSync(file, "utf8");
    } catch {
      return null;
    }
  },
  toplevel: (dir) => git(dir, ["rev-parse", "--show-toplevel"])?.trim() || null,
  portWorktrees: (dir) => parsePortWorktrees(git(dir, ["worktree", "list", "--porcelain"])),
  run: git,
};

// ---------- helpers ----------

function repoRel(abs, deps) {
  const top = deps.toplevel(dirname(abs));
  if (!top) return null;
  const rel = slash(relative(resolve(top), resolve(abs)));
  return rel.startsWith("..") || isAbsolute(rel) ? null : rel;
}

// `rel` is relative to the repository root `top`.
function engineerRel(rel, top, deps) {
  if (ENGINEER_AREAS.some((r) => r.test(rel))) return true;
  const board = BOARD_DIR.exec(rel);
  if (!board || !top) return false;
  const manifest = (deps.readText ?? realDeps.readText)(`${slash(top)}/targets/boards/${board[1]}/Cargo.toml`);
  return manifest === null || dependsOn(manifest, ESP_MCU_PACKAGE);
}

function inEngineerArea(abs, deps) {
  const rel = repoRel(abs, deps);
  return rel !== null && engineerRel(rel, deps.toplevel(dirname(abs)), deps);
}
const dirInEngineerArea = (dir, deps) => inEngineerArea(`${slash(dir)}/x`, deps);

function isPortSession(input, deps, dirs = []) {
  if (String(input.agent_type ?? "").startsWith("dagon:")) return true;
  if (deps.env.DAGON_PORT_SESSION === "1") return true;
  return [...new Set([input.cwd, ...dirs])].some((d) => d && deps.portWorktrees(d).length > 0);
}

// Every directory a shell command may act in: each segment's cwd (after cd /
// git -C) and every path-like argument or redirect target.
function touchedDirs(segments) {
  const dirs = new Set();
  for (const seg of segments) {
    if (seg.cwd === null) continue;
    dirs.add(slash(seg.cwd));
    for (const w of [...seg.words.slice(1), ...seg.targets]) {
      if (!w.startsWith("-") && /[/\\]|^[A-Za-z]:|^\.\.?$/.test(w)) dirs.add(dirname(absPath(seg.cwd, w)));
    }
  }
  return [...dirs];
}

const deny = (reason) => ({
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "deny",
    permissionDecisionReason: reason,
  },
});

// ---------- shell parsing ----------

function isReadOnlyCommand(words) {
  const cmd = String(words[0] ?? "").toLowerCase().replace(/\.exe$/, "");
  if (words.some((w) => /^--output\b/i.test(w))) return false;
  if (cmd === "find") return !words.some((w) => FIND_WRITES.test(w));
  if (READONLY_COMMANDS.has(cmd)) return true;
  if (!(cmd in READONLY_SUBCOMMANDS)) return false;
  const subs = READONLY_SUBCOMMANDS[cmd];
  if (subs === null) return true;
  let i = 1;
  while (i < words.length && /^(-C|-c|\+\S+)$/.test(words[i])) i += words[i].startsWith("+") ? 1 : 2;
  return subs.has(String(words[i] ?? "").toLowerCase());
}

// A directory argument we cannot resolve statically (cd, cd -, cd ~, $VAR).
const isOpaqueDir = (w) => !w || w === "-" || /^~|\$|%/.test(w);
const moveDir = (dir, w) => (isOpaqueDir(w) ? null : isAbsolute(w) || /^[A-Za-z]:/.test(w) ? resolve(w) : dir && resolve(dir, w));

// Splits a command into segments, tracking the working directory through
// cd / pushd / popd. An untrackable directory becomes null (fail closed).
function parseShell(command, cwd) {
  const raw = command
    .replace(/\d*>&\d+/g, " ")
    .replace(/&>/g, ">")
    .split(/&&|\|\||[;|&\r\n]/)
    .map((s) => s.trim())
    .filter(Boolean);
  let dir = cwd ?? ".";
  const stack = [];
  return raw.map((text) => {
    const targets = [...text.matchAll(/\d*>>?\s*([^\s<>]+)/g)]
      .map((m) => unquote(m[1]))
      .filter((t) => !NULL_SINKS.has(t.toLowerCase()));
    const words = text.replace(/\d*>>?\s*[^\s<>]+/g, " ").split(/\s+/).map(unquote).filter(Boolean);
    const cmd = String(words[0] ?? "").toLowerCase();
    if (cmd === "popd") dir = stack.length ? stack.pop() : null;
    else if (cmd === "pushd") {
      stack.push(dir);
      dir = moveDir(dir, words[1]);
    } else if (CD_COMMANDS.has(cmd)) dir = moveDir(dir, words[1]);
    const gitC = cmd === "git" ? words.indexOf("-C") : -1;
    const segCwd = gitC > 0 ? moveDir(dir, words[gitC + 1]) : dir;
    return { text, words, targets, cwd: segCwd, readOnly: isReadOnlyCommand(words) };
  });
}

const GLOB = /[*?[\]{}]/;

function segmentHitsProtected(seg) {
  if (seg.cwd === null) return !(seg.readOnly && seg.targets.length === 0);
  const targetHit = seg.targets.some(
    (t) => PROTECTED_WORD.test(t) || PROTECTED_IN_PATH.test(absPath(seg.cwd, t)),
  );
  if (targetHit) return true;
  if (seg.readOnly) return false;
  const text = seg.text.replace(PACKAGE_FLAG, " ").replace(/["']/g, "");
  if (PROTECTED_WORD.test(text) || PROTECTED_IN_PATH.test(slash(seg.cwd))) return true;
  // Globs and variables that could expand into a protected crate.
  return seg.words.slice(1).some(
    (w) => /\$/.test(w) || (GLOB.test(w) && /(^|\/)crates(\/|$)/i.test(absPath(seg.cwd, w))),
  );
}

function engineerSegmentAllowed(seg, deps) {
  if (seg.cwd === null) return seg.readOnly && seg.targets.length === 0;
  const targetsOk = seg.targets.every((t) => inEngineerArea(absPath(seg.cwd, t), deps));
  if (!targetsOk) return false;
  if (seg.readOnly) return true;
  if (!dirInEngineerArea(seg.cwd, deps)) return false;
  return seg.words.slice(1).every((w) => {
    if (/[$~]/.test(w)) return false;
    if (isAbsolute(w) || /^[A-Za-z]:/.test(w) || w.includes("..")) return inEngineerArea(absPath(seg.cwd, w), deps);
    return true;
  });
}

// ---------- decisions ----------

function decideFile(input, deps) {
  const file = input.tool_input?.file_path ?? input.tool_input?.notebook_path ?? "";
  const abs = absPath(input.cwd, file);
  if (PROTECTED_IN_PATH.test(abs) && isPortSession(input, deps, [dirname(abs)])) return deny(MSG_PROTECTED);
  if (input.agent_type === ENGINEER && !inEngineerArea(abs, deps)) return deny(MSG_ENGINEER);
  return null;
}

function decideShell(input, deps) {
  const command = String(input.tool_input?.command ?? "");
  const segments = parseShell(command, input.cwd);
  const hasSubstitution = SUBSTITUTION.test(command);
  const hitsProtected = segments.some(segmentHitsProtected);
  if ((hasSubstitution || hitsProtected) && isPortSession(input, deps, touchedDirs(segments))) {
    return deny(hitsProtected ? MSG_PROTECTED : MSG_SUBSTITUTION);
  }
  if (input.agent_type === ENGINEER && !segments.every((s) => engineerSegmentAllowed(s, deps))) {
    return deny(MSG_ENGINEER);
  }
  return null;
}

// Returns a deny object, or null to leave the call to the normal permission flow.
export function decide(input, deps = realDeps) {
  const tool = input.tool_name;
  const isFile = FILE_TOOLS.has(tool);
  const isShell = SHELL_TOOLS.has(tool);
  if (!isFile && !isShell) return null;
  if (input.agent_type === JUDGE) {
    if (isFile) return deny(MSG_JUDGE_WRITE);
    return JUDGE_COMMAND.test(String(input.tool_input?.command ?? "")) ? null : deny(MSG_JUDGE_SHELL);
  }
  return isFile ? decideFile(input, deps) : decideShell(input, deps);
}

// Paths from `git status --porcelain -z`: "XY path\0", renames/copies add "\0origpath".
export function parseStatusZ(out) {
  const entries = String(out ?? "").split("\0").filter(Boolean);
  const paths = [];
  for (let i = 0; i < entries.length; i++) {
    paths.push(entries[i].slice(3));
    if (/^[RC]/.test(entries[i])) paths.push(entries[++i]);
  }
  return paths.filter(Boolean);
}

const lines = (out) => String(out ?? "").split(/\r?\n/).map((s) => s.trim()).filter(Boolean);

function changedOutsideRules(root, isPortWorktree, input, deps) {
  const base = deps.run(root, ["merge-base", "HEAD", "develop"])?.trim() || "HEAD";
  const files = [
    ...lines(deps.run(root, ["diff", "--name-only", base, "--", ...PROTECTED])),
    ...lines(deps.run(root, ["ls-files", "--others", "--exclude-standard", "--", ...PROTECTED])),
  ];
  if (input.agent_type === ENGINEER && isPortWorktree) {
    const status = parseStatusZ(deps.run(root, ["status", "--porcelain", "-z", "--untracked-files=all"]));
    files.push(...status.filter((p) => !engineerRel(slash(p), root, deps)));
  }
  return files.map((f) => `${slash(root)}: ${f}`);
}

// PostToolUse / PostToolUseFailure backstop: after a shell call in a port
// session, every repository and port worktree the call could have touched must
// have no change under the protected crates (relative to the fork point from
// develop), and port-engineer must not have changed anything in a port
// worktree outside its port area.
export function postCheck(input, deps = realDeps) {
  if (!SHELL_TOOLS.has(input.tool_name)) return null;
  const dirs = [input.cwd ?? ".", ...touchedDirs(parseShell(String(input.tool_input?.command ?? ""), input.cwd))];
  if (!isPortSession(input, deps, dirs)) return null;
  const portRoots = new Set();
  const roots = new Set();
  for (const d of new Set(dirs)) {
    for (const wt of deps.portWorktrees(d)) portRoots.add(slash(resolve(wt)));
    const top = deps.toplevel(d);
    if (top) roots.add(slash(resolve(top)));
  }
  const offenders = [];
  for (const root of new Set([...portRoots, ...roots])) {
    offenders.push(...changedOutsideRules(root, portRoots.has(root), input, deps));
  }
  if (offenders.length === 0) return null;
  const reason =
    `Port session integrity violation, files changed that must not be: ${[...new Set(offenders)].join(", ")}. ` +
    "Do not try to repair this yourself. Stop the run and report it to a human.";
  const event = input.hook_event_name === "PostToolUseFailure" ? "PostToolUseFailure" : "PostToolUse";
  const base = { continue: false, stopReason: reason, hookSpecificOutput: { hookEventName: event, additionalContext: reason } };
  return event === "PostToolUse" ? { decision: "block", reason, ...base } : base;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const input = JSON.parse(readFileSync(0, "utf8"));
  const result = process.argv[2] === "post" ? postCheck(input) : decide(input);
  if (result) process.stdout.write(JSON.stringify(result));
  process.exit(0);
}
