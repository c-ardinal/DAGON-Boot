// PreToolUse guard for DAGON port sessions.
//
// During a port session the conformance suite, the simulator and the core
// state machine must stay exactly as reviewed, so writes to them are denied.
// A port session is any of:
//   - a call made by one of this plugin's agents (agent_type "dagon:*"),
//   - DAGON_PORT_SESSION=1 in the environment (Agent SDK runs),
//   - the session cwd or the written file lives on a git branch "port/*"
//     (the dagon-port skill runs each attempt on such a branch).
// Additionally port-engineer may only write under targets/esp32s3-*/ and
// conformance-judge may only run `dagon conform` / `dagon sim`.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const PROTECTED_PATH = /(^|\/)crates\/dagon-(conform|sim|core)(\/|$)/i;
const PROTECTED_WORD = /dagon-(conform|sim|core)\b/i;
const PACKAGE_FLAG = /(^|\s)(-p|--package)(\s+|=)\S+/g;
const ENGINEER_PATH = /(^|\/)targets\/esp32s3-[^/]+\//i;
const FILE_TOOLS = new Set(["Write", "Edit", "MultiEdit", "NotebookEdit"]);
const SHELL_TOOLS = new Set(["Bash", "PowerShell"]);
const ENGINEER = "dagon:port-engineer";
const JUDGE = "dagon:conformance-judge";
const PORT_BRANCH_PREFIX = "port/";

const READONLY_COMMANDS = new Set([
  "cat", "head", "tail", "less", "grep", "rg", "ls", "dir", "wc", "diff", "cd", "pwd", "echo",
  "get-content", "select-string", "get-childitem", "test-path",
]);
const READONLY_SUBCOMMANDS = {
  git: new Set(["diff", "log", "show", "status", "blame", "grep", "ls-files", "rev-parse"]),
  cargo: new Set(["build", "check", "test", "clippy", "doc", "tree", "metadata", "run"]),
  dagon: null, // every dagon subcommand only reads the source tree
};

const norm = (p) => String(p ?? "").replace(/\\/g, "/");

function segments(command) {
  return command
    .split(/&&|\|\||[;|\n]/)
    .map((s) => s.trim())
    .filter(Boolean);
}

function words(segment) {
  return segment.split(/\s+/).map((w) => w.replace(/^["']|["']$/g, ""));
}

// Redirections to /dev/null, $null or another fd do not write files.
function hasFileRedirect(command) {
  const stripped = command
    .replace(/\d?>&\d/g, "")
    .replace(/\d?>\s*(\/dev\/null|\$null)/gi, "");
  return /[>]/.test(stripped);
}

function isReadOnlySegment(segment) {
  const [first, second] = words(segment);
  const cmd = first.toLowerCase().replace(/\.exe$/, "");
  if (READONLY_COMMANDS.has(cmd)) return true;
  if (!(cmd in READONLY_SUBCOMMANDS)) return false;
  const subs = READONLY_SUBCOMMANDS[cmd];
  return subs === null || subs.has(String(second ?? "").toLowerCase());
}

function shellWritesProtected(command) {
  const withoutPackages = command.replace(PACKAGE_FLAG, " ");
  if (!PROTECTED_WORD.test(withoutPackages)) return false;
  if (hasFileRedirect(command)) return true;
  return !segments(command).every(isReadOnlySegment);
}

function isJudgeCommand(command) {
  if (hasFileRedirect(command)) return false;
  return segments(command).every((s) => {
    const [first, second] = words(s);
    return /^dagon(\.exe)?$/i.test(first) && /^(conform|sim)$/i.test(String(second ?? ""));
  });
}

function filePathOf(toolInput) {
  return toolInput?.file_path ?? toolInput?.notebook_path ?? "";
}

export function gitBranch(dir) {
  let d = dir;
  while (d && !existsSync(d)) {
    const up = dirname(d);
    if (up === d) return "";
    d = up;
  }
  try {
    return execFileSync("git", ["-C", d, "rev-parse", "--abbrev-ref", "HEAD"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      timeout: 5000,
    }).trim();
  } catch {
    return "";
  }
}

function isPortSession(input, env, branchOf) {
  if (String(input.agent_type ?? "").startsWith("dagon:")) return true;
  if (env.DAGON_PORT_SESSION === "1") return true;
  const dirs = [input.cwd];
  const file = filePathOf(input.tool_input);
  if (file) dirs.push(dirname(resolve(input.cwd ?? ".", file)));
  return dirs.some((d) => d && branchOf(d).startsWith(PORT_BRANCH_PREFIX));
}

const deny = (reason) => ({
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "deny",
    permissionDecisionReason: reason,
  },
});

// Returns a deny object, or null to leave the call to the normal permission flow.
export function decide(input, env = process.env, branchOf = gitBranch) {
  const tool = input.tool_name;
  const agent = input.agent_type ?? "";
  const isFile = FILE_TOOLS.has(tool);
  const isShell = SHELL_TOOLS.has(tool);
  if (!isFile && !isShell) return null;
  const file = norm(filePathOf(input.tool_input));
  const command = String(input.tool_input?.command ?? "");

  const hitsProtected = isFile ? PROTECTED_PATH.test(file) : shellWritesProtected(command);
  if (hitsProtected && isPortSession(input, env, branchOf)) {
    return deny(
      "Port session: crates/dagon-conform, crates/dagon-sim and crates/dagon-core are read-only. " +
        "Fix the target, not the tests. If a test looks wrong, stop and report to a human.",
    );
  }
  if (agent === JUDGE) {
    if (isFile) return deny("conformance-judge has no write access.");
    if (!isJudgeCommand(command)) {
      return deny("conformance-judge may only run `dagon conform` or `dagon sim`.");
    }
  }
  if (agent === ENGINEER && isFile && !ENGINEER_PATH.test(file)) {
    return deny(
      "port-engineer may only edit files under targets/esp32s3-*/. " +
        "Changes to shared crates go to a human, who forwards them to the firmware owners.",
    );
  }
  return null;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const input = JSON.parse(readFileSync(0, "utf8"));
  const result = decide(input);
  if (result) process.stdout.write(JSON.stringify(result));
  process.exit(0);
}
