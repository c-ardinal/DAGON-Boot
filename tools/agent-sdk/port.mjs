#!/usr/bin/env node
// Started by `dagon ai port`: runs the dagon plugin's porting skill with
// the Claude Agent SDK in the DAGON clone. The request comes on stdin and
// progress goes to stdout, one JSON object per line (protocol.mjs). The
// API key is ANTHROPIC_API_KEY in the environment dagon gives this
// program; it is never read from a file here and never printed.
import { query } from "@anthropic-ai/claude-agent-sdk";
import { TOOLS, appended, initProblem, lines, parseRequest, prompt } from "./protocol.mjs";

function say(line) {
  process.stdout.write(`${JSON.stringify(line)}\n`);
}

async function stdin() {
  const parts = [];
  for await (const p of process.stdin) parts.push(p);
  return Buffer.concat(parts).toString("utf8");
}

async function main() {
  const req = parseRequest(await stdin());
  const stop = new AbortController();
  for (const s of ["SIGINT", "SIGTERM"]) process.on(s, () => stop.abort());
  let done = false;
  const run = query({
    prompt: prompt(req),
    options: {
      cwd: req.cwd,
      plugins: [{ type: "local", path: req.plugin }],
      // Only the plugin and the clone's own files: no settings, hooks or
      // skills of this computer's user.
      settingSources: [],
      systemPrompt: { type: "preset", preset: "claude_code", append: appended(req) },
      allowedTools: TOOLS,
      permissionMode: "acceptEdits",
      abortController: stop,
      ...(req.model ? { model: req.model } : {}),
    },
  });
  for await (const message of run) {
    for (const line of lines(message)) {
      if (line.event === "init") {
        const problem = initProblem(line);
        if (problem) {
          stop.abort();
          throw new Error(problem);
        }
      }
      say(line);
      done ||= line.event === "result";
    }
  }
  if (!done) throw new Error("the run ended without a result");
}

main().catch((e) => {
  say({ event: "error", text: e instanceof Error ? e.message : String(e) });
  process.exitCode = 1;
});
