import assert from "node:assert/strict";
import { test } from "node:test";
import { MAX_TEXT, SKILL, appended, initProblem, lines, parseRequest, prompt } from "./protocol.mjs";

const base = { mcu: "esp32s3", board: "m5stopwatch", kind: "V", plugin: "/p", cwd: "/c" };
const req = (more = {}) => parseRequest(JSON.stringify({ ...base, ...more }));

test("the prompt names the skill and its arguments", () => {
  assert.equal(prompt(req()), `/${SKILL} esp32s3 m5stopwatch V`);
  assert.equal(
    prompt(req({ max_hours: 3, max_iterations: 10, rerun: true })),
    `/${SKILL} esp32s3 m5stopwatch V --max-hours 3 --max-iterations 10 --rerun`,
  );
});

test("a request that could change the prompt's arguments is refused", () => {
  for (const more of [
    { mcu: "esp32s3 --rerun" },
    { board: "../x" },
    { mcu: "" },
    { kind: "X" },
    { max_hours: 0 },
    { max_hours: "3" },
    { max_iterations: 1.5 },
    { plugin: "" },
    { description: 3 },
  ]) {
    assert.throws(() => req(more), Error, JSON.stringify(more));
  }
  assert.throws(() => parseRequest("no"), /not JSON/);
  assert.throws(() => parseRequest("[]"), /not an object/);
});

test("the description reaches the system prompt as data, never the arguments", () => {
  const r = req({ description: "USB-C, 16 MiB flash --rerun" });
  assert.equal(prompt(r), `/${SKILL} esp32s3 m5stopwatch V`);
  assert.match(appended(r), /<board-description>\nUSB-C, 16 MiB flash --rerun\n<\/board-description>/);
  assert.doesNotMatch(appended(req()), /board-description/);
});

test("SDK messages become lines", () => {
  const init = lines({
    type: "system",
    subtype: "init",
    plugins: [{ name: "dagon", path: "/p" }],
    skills: [SKILL, "dagon:dagon-topology"],
  });
  assert.deepEqual(init, [{ event: "init", plugins: ["dagon"], skills: [SKILL, "dagon:dagon-topology"] }]);
  assert.equal(initProblem(init[0]), "");
  assert.match(initProblem({ event: "init", plugins: [], skills: [] }), /did not load/);

  const said = lines({
    type: "assistant",
    message: {
      content: [
        { type: "thinking", thinking: "" },
        { type: "text", text: "Reading the datasheet." },
        { type: "tool_use", name: "Bash", input: { command: "cargo build\n--release" } },
        { type: "text", text: "  " },
      ],
    },
  });
  assert.deepEqual(said, [
    { event: "text", text: "Reading the datasheet." },
    { event: "tool", name: "Bash", detail: "cargo build" },
  ]);

  assert.deepEqual(
    lines({ type: "result", subtype: "success", is_error: false, result: "ported", num_turns: 40, total_cost_usd: 1.5 }),
    [{ event: "result", ok: true, text: "ported", turns: 40, cost_usd: 1.5 }],
  );
  assert.deepEqual(lines({ type: "result", subtype: "error_max_turns" }), [
    { event: "result", ok: false, text: "the run ended: error_max_turns", turns: 0, cost_usd: null },
  ]);
  assert.deepEqual(lines({ type: "user" }), []);
  assert.deepEqual(lines(null), []);
});

test("a long text is cut", () => {
  const [line] = lines({ type: "assistant", message: { content: [{ type: "text", text: "x".repeat(MAX_TEXT + 5) }] } });
  assert.equal(line.text.length, MAX_TEXT + 1);
});
