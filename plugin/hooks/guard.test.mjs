// Run with: node --test plugin/hooks/guard.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { decide, dependsOn, parseStatusZ, postCheck } from "./guard.mjs";

// Absolute on the system running the tests: a drive means nothing to Linux's path rules.
const WINDOWS = process.platform === "win32";
const DRIVE = WINDOWS ? "C:" : "";
const ROOT = `${DRIVE}/repo`;
const PORT_WT = `${DRIVE}/w/dagon-port-R`;
const toplevel = (d) => (String(d).replace(/\\/g, "/").toLowerCase().startsWith(PORT_WT.toLowerCase()) ? PORT_WT : ROOT);
const deps = (o = {}) => ({ env: {}, toplevel, portWorktrees: () => [], run: () => "", ...o });
const offSession = deps();
const inSession = deps({ portWorktrees: () => [PORT_WT] });

const call = (tool, tool_input, extra = {}) => ({ tool_name: tool, tool_input, cwd: ROOT, ...extra });
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
  expectDeny(wr(`${ROOT}/crates/dagon-conform/src/lib.rs`, engineer), offSession, /read-only/);
});

test("port-engineer Write to targets/esp32s3-* is allowed", () => {
  expectPass(wr(`${ROOT}/targets/esp32s3-watch/src/lib.rs`, engineer), offSession);
});

test("port-engineer area follows the MCU / board / samples layout", () => {
  const manifests = {
    [`${ROOT}/targets/boards/m5stopwatch/Cargo.toml`]: '[dependencies]\ndagon-mcu-esp32s3 = { path = "../../mcu/esp32s3" }\n',
    [`${ROOT}/targets/boards/rp2040-touch-lcd/Cargo.toml`]: '[dependencies]\ndagon-mcu-rp2040 = { path = "../../mcu/rp2040" }\n',
  };
  const d = deps({ readText: (f) => manifests[f] ?? null });
  expectPass(wr(`${ROOT}/targets/mcu/esp32s3/src/node.rs`, engineer), d);
  expectPass(wr(`${ROOT}/targets/boards/m5stopwatch/src/lib.rs`, engineer), d);
  expectPass(wr(`${ROOT}/targets/boards/new-s3-board/Cargo.toml`, engineer), d);
  expectPass(wr(`${ROOT}/samples/esp32s3-app/src/main.rs`, engineer), d);
  expectDeny(wr(`${ROOT}/targets/boards/rp2040-touch-lcd/src/lib.rs`, engineer), d, /may only edit/);
  expectDeny(wr(`${ROOT}/targets/mcu/rp2040/src/lib.rs`, engineer), d, /may only edit/);
  expectDeny(wr(`${ROOT}/samples/rp2040-hub/src/main.rs`, engineer), d, /may only edit/);
  expectPass(sh("cd targets/mcu/esp32s3 && sed -i s/a/b/ src/lib.rs", engineer), d);
  expectDeny(sh("cd targets/mcu/rp2040 && sed -i s/a/b/ src/lib.rs", engineer), d);

  const status =
    " M targets/mcu/esp32s3/src/node.rs\0 M targets/boards/m5stopwatch/src/lib.rs\0 M targets/boards/rp2040-touch-lcd/src/lib.rs\0";
  const post = deps({
    portWorktrees: () => [PORT_WT],
    readText: (f) => manifests[f.replace(PORT_WT, ROOT)] ?? null,
    run: (_r, a) => (a[0] === "status" ? status : ""),
  });
  const r = postCheck(sh("cargo fmt", engineer), post);
  assert.equal(r.decision, "block");
  assert.match(r.reason, /rp2040-touch-lcd/);
  assert.doesNotMatch(r.reason, /esp32s3|m5stopwatch/);
});

test("dependsOn reads dependency tables, not comments or other tables", () => {
  const pkg = "dagon-mcu-esp32s3";
  assert.ok(dependsOn('[dependencies]\ndagon-mcu-esp32s3 = { path = "x" }\n', pkg));
  assert.ok(dependsOn("[target.'cfg(target_arch = \"xtensa\")'.dependencies]\ndagon-mcu-esp32s3 = \"0.1\"\n", pkg));
  assert.ok(dependsOn('[dependencies.dagon-mcu-esp32s3]\npath = "x"\n', pkg));
  assert.ok(!dependsOn("# needs dagon-mcu-esp32s3 later\n[dependencies]\nesp-hal = \"1\"\n", pkg));
  assert.ok(!dependsOn('[dev-dependencies]\ndagon-mcu-esp32s3 = { path = "x" }\n', pkg));
  assert.ok(!dependsOn('[features]\nx = ["dagon-mcu-esp32s3/conform"]\n', pkg));
  assert.ok(!dependsOn('[dependencies]\ndagon-mcu-esp32s3-extra = "1"\n', pkg));
  assert.ok(!dependsOn('[package.metadata.dagon]\nmcu = "esp32s3"\n', pkg));
});

test("outside a port session protected crates are writable", () => {
  expectPass(wr(`${ROOT}/crates/dagon-conform/x.rs`), offSession);
  expectPass(sh("sed -i s/a/b/ crates/dagon-core/src/lib.rs"), offSession);
});

test("DAGON_PORT_SESSION=1 marks a port session", () => {
  expectDeny(wr(`${ROOT}/crates/dagon-sim/x.rs`), deps({ env: { DAGON_PORT_SESSION: "1" } }), /read-only/);
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
  expectDeny(wr(`${ROOT}/targets/esp32s3-x/a.rs`, judge), offSession);
});

test("B2 port session: single &, substitutions and protected cwd", () => {
  expectDeny(sh("cat a & rm -rf crates/dagon-core"), inSession, /read-only/);
  expectDeny(sh("echo $(rm -rf crates/dagon-core)"), inSession);
  expectDeny(sh("echo `rm -rf x`"), inSession);
  expectDeny(sh("sed -i s/a/b/ src/lib.rs", { cwd: `${ROOT}/crates/dagon-conform` }), inSession, /read-only/);
  expectDeny(sh("cd crates/dagon-conform && sed -i s/a/b/ src/lib.rs"), inSession, /read-only/);
});

test("B3 port worktree is a sibling of the session cwd", () => {
  expectDeny(sh("cd ../dagon-port-R && sed -i s/a/b/ crates/dagon-conform/src/lib.rs"), inSession);
  expectDeny(sh(`sed -i s/a/b/ ${PORT_WT}/crates/dagon-conform/src/lib.rs`), inSession);
  expectDeny(sh("git -C ../dagon-port-R checkout develop -- crates/dagon-sim"), inSession);
  expectDeny(wr(`${PORT_WT}/crates/dagon-core/src/lib.rs`), inSession);
});

test("B4 paths are normalised and anchored at the repository root", () => {
  for (const f of [
    `${ROOT}/targets/esp32s3-a/../../Cargo.toml`,
    `${ROOT}/targets/esp32s3-a/../../crates/dagon-port/src/lib.rs`,
    `${ROOT}/crates/dagon-port/targets/esp32s3-x/a.rs`,
  ]) expectDeny(wr(f, engineer), offSession);
  for (const f of [
    `${ROOT}/crates//dagon-core/src/lib.rs`,
    `${ROOT}/crates/./dagon-core/src/lib.rs`,
    ...(WINDOWS ? ["C:\\repo\\crates\\\\dagon-core\\lib.rs"] : []),
  ]) expectDeny(wr(f), inSession, /read-only/);
  expectDeny(wr("src/lib.rs", { cwd: `${ROOT}/crates/dagon-conform` }), inSession, /read-only/);
});

test("O2 port-engineer shell writes stay inside targets/esp32s3-*", () => {
  expectDeny(sh("sed -i s/a/b/ crates/dagon-port/src/lib.rs", engineer), offSession);
  expectDeny(sh(`cd ${ROOT}/targets/esp32s3-x && sed -i s/a/b/ ../../crates/dagon-port/src/lib.rs`, engineer), offSession);
  expectDeny(sh(`cd ${ROOT}/targets/esp32s3-x && rm -rf $HOME/x`, engineer), offSession);
  expectDeny(sh(`echo x > ${ROOT}/Cargo.toml`, engineer), offSession);
  expectPass(sh(`cd ${ROOT}/targets/esp32s3-x && sed -i s/a/b/ src/lib.rs`, engineer), offSession);
  expectPass(sh(`cd ${ROOT}/targets/esp32s3-x && cargo build --release 2>&1`, engineer), offSession);
  expectPass(sh("cat crates/dagon-port/src/lib.rs", engineer), offSession);
});

test("O3 read-only commands touching protected crates pass", () => {
  expectPass(sh("cat crates/dagon-conform/src/lib.rs"), inSession);
  expectPass(sh("cargo test -p dagon-conform 2>&1"), inSession);
  expectPass(sh("git diff crates/dagon-core"), inSession);
  expectPass(sh("git -C crates/dagon-core status"), inSession);
  expectPass(sh("find crates/dagon-core -name '*.rs'"), inSession);
  expectPass(sh(`cat crates/dagon-core/src/lib.rs > ${DRIVE}/tmp/x`), inSession);
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
  const status = " M targets/esp32s3-x/src/lib.rs\0 M Cargo.toml\0";
  const d = deps({ portWorktrees: () => [PORT_WT], run: (_r, a) => (a[0] === "status" ? status : "") });
  const r = postCheck(sh("cargo fmt", engineer), d);
  assert.equal(r.decision, "block");
  assert.match(r.reason, /Cargo\.toml/);
  assert.doesNotMatch(r.reason, /esp32s3-x/);
});

test("popd, opaque cd, globs, quotes and variables fail closed", () => {
  expectDeny(sh("pushd targets/esp32s3-a && popd && rm Cargo.toml", engineer), offSession);
  expectDeny(sh("cd targets/esp32s3-a && cd - && rm Cargo.toml", engineer), offSession);
  expectDeny(sh("cd ~ && rm -rf x"), inSession);
  expectDeny(sh("rm -rf crates/dagon-c*re"), inSession);
  expectDeny(sh("cd crates && rm -rf dagon-{core,sim}"), inSession);
  expectDeny(sh('rm -rf crates/dagon-"core"'), inSession);
  expectDeny(sh("D=crates/dagon-co; rm -rf ${D}re"), inSession);
  expectPass(sh("cd targets/esp32s3-a && rm -f build/*.o"), inSession);
  expectPass(sh("pushd targets/esp32s3-a && cargo build && popd"), inSession);
});

test("parseStatusZ keeps the leading status column and rename origins", () => {
  assert.deepEqual(parseStatusZ(" M targets/esp32s3-a/src/lib.rs\0R  new.rs\0old.rs\0?? x y.rs\0"), [
    "targets/esp32s3-a/src/lib.rs",
    "new.rs",
    "old.rs",
    "x y.rs",
  ]);
});

// ---------- real git (no stubs) ----------

function scratchRepo() {
  const base = mkdtempSync(join(tmpdir(), "dagon-guard-"));
  const repo = join(base, "repo");
  const g = (cwd, ...args) => execFileSync("git", ["-C", cwd, ...args], { stdio: "pipe" });
  for (const f of ["crates/dagon-core/src/lib.rs", "crates/dagon-sim/src/lib.rs", "targets/esp32s3-a/src/lib.rs", "Cargo.toml"]) {
    mkdirSync(join(repo, f, ".."), { recursive: true });
    writeFileSync(join(repo, f), "orig\n");
  }
  g(repo, "init", "-q", "-b", "develop");
  g(repo, "add", ".");
  g(repo, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init");
  const port = join(base, "dagon-port-R");
  g(repo, "worktree", "add", "-q", port, "-b", "port/R");
  return { base, repo, port, cleanup: () => rmSync(base, { recursive: true, force: true }) };
}

test("N1 real git: legit engineer edit of a committed target file passes the backstop", (t) => {
  const s = scratchRepo();
  t.after(s.cleanup);
  writeFileSync(join(s.port, "targets/esp32s3-a/src/lib.rs"), "engineer edit\n");
  const input = sh("cargo build", { cwd: join(s.port, "targets/esp32s3-a"), agent_type: "dagon:port-engineer" });
  assert.equal(postCheck(input), null);
  writeFileSync(join(s.port, "Cargo.toml"), "outside\n");
  assert.match(postCheck(input).reason, /Cargo\.toml/);
});

test("N2 real git: non-repo cwd still sees the port worktree", (t) => {
  const s = scratchRepo();
  t.after(s.cleanup);
  const P = s.port.replace(/\\/g, "/");
  const cwd = { cwd: s.base };
  expectDeny(sh(`cd ${P} && sed -i s/a/b/ crates/dagon-core/src/lib.rs`, cwd), undefined, /read-only/);
  expectDeny(sh(`sed -i s/a/b/ ${P}/crates/dagon-core/src/lib.rs`, cwd), undefined, /read-only/);
  expectDeny(sh(`git -C ${P} checkout develop -- crates/dagon-sim`, cwd), undefined, /read-only/);
  expectDeny(wr(`${P}/crates/dagon-core/src/lib.rs`, cwd), undefined, /read-only/);
  expectPass(sh(`ls ${P}`, cwd));

  const tamper = sh(`cp x ${P}/crates/dagon-c*/src/`, cwd);
  assert.equal(postCheck(tamper), null);
  writeFileSync(join(s.port, "crates/dagon-core/src/lib.rs"), "tampered\n");
  const r = postCheck(tamper);
  assert.equal(r.decision, "block");
  assert.equal(r.continue, false);
  assert.match(r.reason, /crates\/dagon-core\/src\/lib\.rs/);

  const f = postCheck({ ...tamper, hook_event_name: "PostToolUseFailure" });
  assert.equal(f.continue, false);
  assert.equal(f.hookSpecificOutput.hookEventName, "PostToolUseFailure");
  assert.equal(f.decision, undefined);
});
