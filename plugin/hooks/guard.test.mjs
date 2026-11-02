// Run with: node --test plugin/hooks/guard.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { decide } from "./guard.mjs";

const offBranch = () => "develop";
const portBranch = () => "port/esp32s3-watch-1";
const call = (tool, tool_input, extra = {}) => ({ tool_name: tool, tool_input, cwd: "/repo", ...extra });
const denied = (r) => r?.hookSpecificOutput?.permissionDecision === "deny";
const engineer = { agent_type: "dagon:port-engineer" };
const judge = { agent_type: "dagon:conformance-judge" };

test("port-engineer Write to dagon-conform is denied", () => {
  const r = decide(call("Write", { file_path: "/repo/crates/dagon-conform/src/lib.rs" }, engineer), {}, offBranch);
  assert.ok(denied(r));
  assert.match(r.hookSpecificOutput.permissionDecisionReason, /read-only/);
});

test("port-engineer Write to targets/esp32s3-* is allowed", () => {
  const r = decide(call("Write", { file_path: "/repo/targets/esp32s3-watch/src/lib.rs" }, engineer), {}, offBranch);
  assert.equal(r, null);
});

test("port-engineer Write outside targets/esp32s3-* is denied", () => {
  assert.ok(denied(decide(call("Edit", { file_path: "/repo/crates/dagon-port/src/lib.rs" }, engineer), {}, offBranch)));
  assert.ok(denied(decide(call("Write", { file_path: "/repo/targets/stm32f401-x/a.rs" }, engineer), {}, offBranch)));
});

test("main thread on a port/* branch cannot edit protected crates", () => {
  for (const c of ["conform", "sim", "core"]) {
    const r = decide(call("Edit", { file_path: `C:\\w\\crates\\dagon-${c}\\src\\lib.rs` }), {}, portBranch);
    assert.ok(denied(r), c);
  }
});

test("DAGON_PORT_SESSION=1 marks a port session", () => {
  const r = decide(call("Write", { file_path: "/repo/crates/dagon-sim/x.rs" }), { DAGON_PORT_SESSION: "1" }, offBranch);
  assert.ok(denied(r));
});

test("outside a port session protected crates are writable", () => {
  assert.equal(decide(call("Write", { file_path: "/repo/crates/dagon-conform/x.rs" }), {}, offBranch), null);
});

test("shell writes to protected crates are denied, reads and builds pass", () => {
  const sh = (command) => decide(call("Bash", { command }), {}, portBranch);
  assert.ok(denied(sh("sed -i s/a/b/ crates/dagon-conform/src/lib.rs")));
  assert.ok(denied(sh("echo x > crates/dagon-core/src/lib.rs")));
  assert.ok(denied(sh("cd crates && rm -rf dagon-sim")));
  assert.ok(denied(sh("git checkout -- crates/dagon-conform")));
  assert.equal(sh("cat crates/dagon-conform/src/lib.rs"), null);
  assert.equal(sh("cargo test -p dagon-conform 2>&1"), null);
  assert.equal(sh("git diff crates/dagon-core"), null);
});

test("conformance-judge may only run dagon conform / dagon sim", () => {
  assert.equal(decide(call("Bash", { command: "dagon conform --json" }, judge), {}, offBranch), null);
  assert.equal(decide(call("Bash", { command: "dagon sim run s1" }, judge), {}, offBranch), null);
  assert.ok(denied(decide(call("Bash", { command: "cargo build" }, judge), {}, offBranch)));
  assert.ok(denied(decide(call("Bash", { command: "dagon conform > out.json" }, judge), {}, offBranch)));
  assert.ok(denied(decide(call("Write", { file_path: "/repo/targets/esp32s3-x/a.rs" }, judge), {}, offBranch)));
});
