// Run with: node --test plugin/hooks/guard.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { decide, postCheck } from "./guard.mjs";

const PORT_WT = "C:/w/dagon-port-R";
const toplevel = (d) => (String(d).replace(/\\/g, "/").toLowerCase().startsWith("c:/w/dagon-port-r") ? PORT_WT : "C:/repo");
const deps = (o = {}) => ({ env: {}, toplevel, portWorktrees: () => [], run: () => "", ...o });
const offSession = deps();
const inSession = deps({ portWorktrees: () => [PORT_WT] });

const call = (tool, tool_input, extra = {}) => ({ tool_name: tool, tool_input, cwd: "C:/repo", ...extra });
const sh = (command, extra) => call("Bash", { command }, extra);
const wr = (file_path, extra) => call("Write", { file_path, content: "x" }, extra);
const denied = (r) => r?.hookSpecificOutput?.permissionDecision === "deny";
const engineer = { agent_type: "dagon:port-engineer" };
const judge = { agent_type: "dagon:conformance-judge" };

const expectDeny = (input, d, re) => {
  const r = decide(input, d);
  assert.ok(denied(r), JSON.stringify(input.tool_input));
  if (re) assert.match(r.hookSpecificOutput.permissionDecisionReason, re);
};
const expectPass = (input, d) => assert.equal(decide(input, d), null, JSON.stringify(input.tool_input));

test("port-engineer Write to dagon-conform is denied as read-only", () => {
  expectDeny(wr("C:/repo/crates/dagon-conform/src/lib.rs", engineer), offSession, /read-only/);
});

test("port-engineer Write to targets/esp32s3-* is allowed", () => {
  expectPass(wr("C:/repo/targets/esp32s3-watch/src/lib.rs", engineer), offSession);
});

test("outside a port session protected crates are writable", () => {
  expectPass(wr("C:/repo/crates/dagon-conform/x.rs"), offSession);
  expectPass(sh("sed -i s/a/b/ crates/dagon-core/src/lib.rs"), offSession);
});

test("DAGON_PORT_SESSION=1 marks a port session", () => {
  expectDeny(wr("C:/repo/crates/dagon-sim/x.rs"), deps({ env: { DAGON_PORT_SESSION: "1" } }), /read-only/);
});

test("B1 judge: only a bare dagon conform / dagon sim command", () => {
  expectPass(sh("dagon conform --json", judge), offSession);
  expectPass(sh("dagon sim run s1", judge), offSession);
  for (const c of [
    "dagon sim s1 & rm -rf crates/dagon-core",
    "dagon sim $(rm -rf targets)",
    "dagon sim `rm -rf targets`",
    "dagon conform > out.json",
    "dagon conform\nrm -rf targets",
    "dagon sim s1; rm x",
    "dagon sim s1 | tee x",
    "cargo build",
  ]) expectDeny(sh(c, judge), offSession);
  expectDeny(wr("C:/repo/targets/esp32s3-x/a.rs", judge), offSession);
});

test("B2 port session: single &, substitutions and protected cwd", () => {
  expectDeny(sh("cat a & rm -rf crates/dagon-core"), inSession, /read-only/);
  expectDeny(sh("echo $(rm -rf crates/dagon-core)"), inSession);
  expectDeny(sh("echo `rm -rf x`"), inSession);
  expectDeny(sh("sed -i s/a/b/ src/lib.rs", { cwd: "C:/repo/crates/dagon-conform" }), inSession, /read-only/);
  expectDeny(sh("cd crates/dagon-conform && sed -i s/a/b/ src/lib.rs"), inSession, /read-only/);
});

test("B3 port worktree is a sibling of the session cwd", () => {
  expectDeny(sh("cd ../dagon-port-R && sed -i s/a/b/ crates/dagon-conform/src/lib.rs"), inSession);
  expectDeny(sh("sed -i s/a/b/ C:/w/dagon-port-R/crates/dagon-conform/src/lib.rs"), inSession);
  expectDeny(sh("git -C ../dagon-port-R checkout develop -- crates/dagon-sim"), inSession);
  expectDeny(wr("C:/w/dagon-port-R/crates/dagon-core/src/lib.rs"), inSession);
});

test("B4 paths are normalised and anchored at the repository root", () => {
  for (const f of [
    "C:/repo/targets/esp32s3-a/../../Cargo.toml",
    "C:/repo/targets/esp32s3-a/../../crates/dagon-port/src/lib.rs",
    "C:/repo/crates/dagon-port/targets/esp32s3-x/a.rs",
  ]) expectDeny(wr(f, engineer), offSession);
  for (const f of [
    "C:/repo/crates//dagon-core/src/lib.rs",
    "C:/repo/crates/./dagon-core/src/lib.rs",
    "C:\\repo\\crates\\\\dagon-core\\lib.rs",
  ]) expectDeny(wr(f), inSession, /read-only/);
  expectDeny(wr("src/lib.rs", { cwd: "C:/repo/crates/dagon-conform" }), inSession, /read-only/);
});

test("O2 port-engineer shell writes stay inside targets/esp32s3-*", () => {
  expectDeny(sh("sed -i s/a/b/ crates/dagon-port/src/lib.rs", engineer), offSession);
  expectDeny(sh("cd C:/repo/targets/esp32s3-x && sed -i s/a/b/ ../../crates/dagon-port/src/lib.rs", engineer), offSession);
  expectDeny(sh("cd C:/repo/targets/esp32s3-x && rm -rf $HOME/x", engineer), offSession);
  expectDeny(sh("echo x > C:/repo/Cargo.toml", engineer), offSession);
  expectPass(sh("cd C:/repo/targets/esp32s3-x && sed -i s/a/b/ src/lib.rs", engineer), offSession);
  expectPass(sh("cd C:/repo/targets/esp32s3-x && cargo build --release 2>&1", engineer), offSession);
  expectPass(sh("cat crates/dagon-port/src/lib.rs", engineer), offSession);
});

test("O3 read-only commands touching protected crates pass", () => {
  expectPass(sh("cat crates/dagon-conform/src/lib.rs"), inSession);
  expectPass(sh("cargo test -p dagon-conform 2>&1"), inSession);
  expectPass(sh("git diff crates/dagon-core"), inSession);
  expectPass(sh("git -C crates/dagon-core status"), inSession);
  expectPass(sh("find crates/dagon-core -name '*.rs'"), inSession);
  expectPass(sh("cat crates/dagon-core/src/lib.rs > C:/tmp/x"), inSession);
  expectDeny(sh("find crates/dagon-core -delete"), inSession);
  expectDeny(sh("git diff --output=crates/dagon-core/x HEAD"), inSession);
  expectDeny(sh("git checkout -- crates/dagon-conform"), inSession);
});

test("backstop: PostToolUse blocks when protected crates changed", () => {
  const changed = deps({
    portWorktrees: () => [PORT_WT],
    run: (_root, args) => (args[0] === "diff" ? "crates/dagon-core/src/lib.rs" : ""),
  });
  const r = postCheck(sh("cd crates && cp x dagon-c*/"), changed);
  assert.equal(r.decision, "block");
  assert.equal(r.continue, false);
  assert.match(r.reason, /crates\/dagon-core\/src\/lib\.rs/);
  assert.equal(postCheck(sh("ls"), inSession), null);
  assert.equal(postCheck(sh("ls"), offSession), null);
});

test("backstop: port-engineer changes outside targets/esp32s3-* are blocked", () => {
  const status = " M targets/esp32s3-x/src/lib.rs\n M Cargo.toml\n";
  const d = deps({ portWorktrees: () => [PORT_WT], run: (_r, a) => (a[0] === "status" ? status : "") });
  const r = postCheck(sh("cargo fmt", engineer), d);
  assert.equal(r.decision, "block");
  assert.match(r.reason, /Cargo\.toml/);
  assert.doesNotMatch(r.reason, /esp32s3-x/);
});
