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
// Additionally port-engineer may only write under targets/esp32s3-*/ and
// conformance-judge may only run `dagon conform` / `dagon sim`.
//
// ponytail: shell commands are judged by string inspection (fail-closed on
// substitutions); globs, patch tools and build scripts are caught afterwards
// by the PostToolUse diff check, not prevented.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const PROTECTED = ["crates/dagon-conform", "crates/dagon-sim", "crates/dagon-core"];
const PROTECTED_IN_PATH = /(^|\/)crates\/dagon-(conform|sim|core)(\/|$)/i;
const PROTECTED_WORD = /dagon-(conform|sim|core)\b/i;
const PACKAGE_FLAG = /(^|\s)(-p|--package)(\s+|=)\S+/g;
const ENGINEER_REL = /^targets\/esp32s3-[^/]+\//i;
const SUBSTITUTION = /\$\(|`|<\(|>\(/;
const JUDGE_COMMAND = /^[ \t]*dagon(\.exe)?[ \t]+(conform|sim)([ \t][^&|;$`<>(){}\r\n]*)?$/i;
const FILE_TOOLS = new Set(["Write", "Edit", "MultiEdit", "NotebookEdit"]);
const SHELL_TOOLS = new Set(["Bash", "PowerShell"]);
const ENGINEER = "dagon:port-engineer";
const JUDGE = "dagon:conformance-judge";
const CD_COMMANDS = new Set(["cd", "pushd", "chdir", "set-location", "sl"]);
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
  "port-engineer may only edit files under targets/esp32s3-*/ (shell writes: cd into the target directory first, " +
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
  while (d && !existsSync(d)) {
    const up = dirname(d);
    if (up === d) return null;
    d = up;
  }
  try {
    return execFileSync("git", ["-C", d, ...args], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      timeout: 5000,
    }).trim();
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
  toplevel: (dir) => git(dir, ["rev-parse", "--show-toplevel"]),
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

const inEngineerArea = (abs, deps) => ENGINEER_REL.test(repoRel(abs, deps) ?? "");
const dirInEngineerArea = (dir, deps) => inEngineerArea(`${slash(dir)}/x`, deps);

function isPortSession(input, deps, dirs = []) {
  if (String(input.agent_type ?? "").startsWith("dagon:")) return true;
  if (deps.env.DAGON_PORT_SESSION === "1") return true;
  return [input.cwd, ...dirs].some((d) => d && deps.portWorktrees(d).length > 0);
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

// Splits a command into segments, tracking the working directory through cd.
function parseShell(command, cwd) {
  const raw = command
    .replace(/\d*>&\d+/g, " ")
    .replace(/&>/g, ">")
    .split(/&&|\|\||[;|&\r\n]/)
    .map((s) => s.trim())
    .filter(Boolean);
  let dir = cwd ?? ".";
  return raw.map((text) => {
    const targets = [...text.matchAll(/\d*>>?\s*([^\s<>]+)/g)]
      .map((m) => unquote(m[1]))
      .filter((t) => !NULL_SINKS.has(t.toLowerCase()));
    const words = text.replace(/\d*>>?\s*[^\s<>]+/g, " ").split(/\s+/).map(unquote).filter(Boolean);
    const cmd = String(words[0] ?? "").toLowerCase();
    if (CD_COMMANDS.has(cmd) && words[1]) dir = resolve(dir, words[1]);
    const gitC = cmd === "git" ? words.indexOf("-C") : -1;
    const segCwd = gitC > 0 && words[gitC + 1] ? resolve(dir, words[gitC + 1]) : dir;
    return { text, words, targets, cwd: segCwd, readOnly: isReadOnlyCommand(words) };
  });
}

function segmentHitsProtected(seg) {
  const targetHit = seg.targets.some(
    (t) => PROTECTED_WORD.test(t) || PROTECTED_IN_PATH.test(absPath(seg.cwd, t)),
  );
  if (targetHit) return true;
  if (seg.readOnly) return false;
  return PROTECTED_WORD.test(seg.text.replace(PACKAGE_FLAG, " ")) || PROTECTED_IN_PATH.test(slash(seg.cwd));
}

function engineerSegmentAllowed(seg, deps) {
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
  if ((hasSubstitution || hitsProtected) && isPortSession(input, deps)) {
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

// PostToolUse backstop: after a shell call in a port session, every port
// worktree must have no change under the protected crates (relative to its
// fork point from develop), and port-engineer must not have changed anything
// outside targets/esp32s3-*/.
export function postCheck(input, deps = realDeps) {
  if (!SHELL_TOOLS.has(input.tool_name) || !isPortSession(input, deps)) return null;
  const roots = new Set(deps.portWorktrees(input.cwd ?? "."));
  const top = deps.toplevel(input.cwd ?? ".");
  if (top) roots.add(top);
  const offenders = [];
  for (const root of roots) {
    const base = deps.run(root, ["merge-base", "HEAD", "develop"]) || "HEAD";
    const lines = [
      deps.run(root, ["diff", "--name-only", base, "--", ...PROTECTED]),
      deps.run(root, ["ls-files", "--others", "--exclude-standard", "--", ...PROTECTED]),
    ];
    if (input.agent_type === ENGINEER) {
      const status = String(deps.run(root, ["status", "--porcelain", "--untracked-files=all"]) ?? "");
      const paths = status.split(/\r?\n/).filter(Boolean).flatMap((l) => l.slice(3).split(" -> "));
      lines.push(paths.filter((p) => !ENGINEER_REL.test(slash(unquote(p)))).join("\n"));
    }
    const files = lines.join("\n").split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
    offenders.push(...files.map((f) => `${slash(root)}: ${f}`));
  }
  if (offenders.length === 0) return null;
  const reason =
    `Port session integrity violation, files changed that must not be: ${[...new Set(offenders)].join(", ")}. ` +
    "Do not try to repair this yourself. Stop the run and report it to a human.";
  return { decision: "block", reason, continue: false, stopReason: reason };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const input = JSON.parse(readFileSync(0, "utf8"));
  const result = process.argv[2] === "post" ? postCheck(input) : decide(input);
  if (result) process.stdout.write(JSON.stringify(result));
  process.exit(0);
}
