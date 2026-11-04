# DAGON agents: serve API contract (UX3C)

DAGON talks to an agent through one interface. Agents are data: an agent
(id, name, provider) and the ways to reach it, each of a transport kind DAGON
implements. The builtin set is Claude (Anthropic) with two ways: `claude-code`
(Claude Code headless with the user's own login, the default) and `api` (the
Messages API with `ANTHROPIC_API_KEY`). The user consents once, when linking,
to one agent and one way; after that questions are sent without asking again.
Logins and keys stay with the agent or in the CLI's environment and never reach
the page.

All routes below are under `dagon serve` with the same guards as every `/v1/`
route: Host and Origin checks (403), `Authorization: Bearer <passphrase>` (401).

## GET /v1/agent

```json
{
  "agents": [
    { "id": "claude", "name": "Claude", "provider": "Anthropic",
      "ways": [
        { "id": "claude-code", "name": "Claude Code", "kind": "command", "available": true },
        { "id": "api", "name": "API キー", "kind": "messages-http", "available": false }
      ] }
  ],
  "default": { "agent": "claude", "way": "claude-code" },
  "linked": false,
  "consent": null,
  "available": true,
  "provider": { "id": "claude", "name": "Claude (Anthropic)" },
  "errors": [],
  "sends": [
    "構成と定義の事実（ノード・リンク・部品・設定値）",
    "選んだノードやリンク",
    "確かめた結果（問題・◇ 未対応・更新の記録）"
  ],
  "never_sends": ["鍵", "コマンドとその出力の全文", "ファイルのパス"]
}
```

- `agents`: builtin first, then the user's own (`~/.dagon/agents/*.toml`).
  Each way has `kind` (`command` | `messages-http`) and `available`: whether it
  can be used on this machine now (a `command` way: its program is on PATH; a
  `messages-http` way: its key variable is set). No program, argument, URL,
  key variable or key is ever in this body.
- `default`: the way the linking dialog preselects (first agent, first way).
- `linked`: a consent is recorded for an agent and way that are still defined.
- `consent`: `{ "agent": "claude", "way": "claude-code", "at": "2026-11-04T04:14:35Z" }`
  once linked (`at` is UTC, RFC 3339), else `null`.
- `available` / `provider`: the linked way (or the default way before
  linking): whether it can be used here, and the agent's name for the linking
  texts (`"<name> (<provider>)"`). DAGON can answer only when `linked` and
  `available` are both true.
- `errors`: user definition files that did not load, as
  `"<file name>: <why>"` (never a folder).
- `sends` / `never_sends`: the lists the linking dialog shows (from the round8c
  string table). The terminal (`dagon ai link`) shows the same lists.

## POST /v1/agent/link

Body: `{ "agent": "claude", "way": "claude-code", "consent": true }`. Records
the consent for that agent and way (a new link replaces the old one; changing
the way asks for consent again).

- 200: the same body as GET /v1/agent (now `linked: true`).
- 400 `agent and way must name a defined agent and way` /
  `consent must be true` (only the JSON boolean `true` counts); nothing is
  recorded. The round 1 body `{provider: "anthropic", ...}` is a 400.
- 403 in `dagon demo` (linking is refused there; GET still works).
- 500 `the consent could not be saved`.

Linking does not need the way to be available (the user may install Claude
Code or set the key afterwards).

## DELETE /v1/agent/link

Withdraws the consent (also fine when none was recorded).

- 200: the same body as GET /v1/agent (now `linked: false`).
- 403 in `dagon demo`.
- **Unlinking waits for an agent call in flight.** Every call holds the
  consent from its check until the call ends (one lock in the process, and
  `~/.dagon/agent.lock` between `dagon serve` and `dagon ai unlink`); DELETE
  takes that lock, removes the consent and only then answers 200. The wait is
  at most one call: up to about 120 s for a question, up to 600 s for a round
  of a topology draft. The page should show a waiting state until DELETE
  answers.
- After that no call passes the check: running `ai_topology` jobs are
  cancelled, a question being answered makes no further call and answers 409
  `AGENT_NOT_LINKED` instead of its answer (`dagon ai ask` does not print it
  either).

## AI routes before linking

Every `/v1/ai/...` route (today `POST /v1/ai/topology`, `POST /v1/ai/port`)
answers, until linked:

```json
HTTP 409
{ "error": "DAGON is not linked to an agent; link it first (consent is asked once)",
  "code": "AGENT_NOT_LINKED" }
```

Match on `code`, not on the text. Once linked the routes behave as before
(`/v1/ai/topology` starts a job; a way that cannot be used here fails inside
the job with its reason, e.g. `ANTHROPIC_API_KEY is not set`).

In `dagon demo` every route that would reach an agent answers 403
(`/v1/agent/link` POST/DELETE, `/v1/agent/ask`, `/v1/ai/...`): a demo never
calls a real agent. `GET /v1/agent` still answers, and `ai_topology` is left out
of the demo's `features`.

`GET /v1/hello` keeps `api_key` (same value as `available` of GET /v1/agent;
the name is kept for the page).

## POST /v1/agent/ask

One question to DAGON, answered in the same request (no job, no events).

Body:

```json
{
  "question": "これは何が問題？",
  "kind": "problem",
  "selection": { "node": "seg" },
  "history": [
    { "role": "user", "text": "次は？" },
    { "role": "dagon", "text": "ビルドします。" }
  ]
}
```

- `question` (required): 1 to 2000 characters, not only spaces. For the four
  fixed questions send their label (`これは何が問題？` and so on) together with `kind`.
- `kind` (optional, default `free`): `problem` | `next` | `propose` | `failure` | `free`.
- `selection` (optional, null for the whole 構成): `{ "node": "<id>" }` or
  `{ "link": { "from": "<id>", "to": "<id>" } }`. Ids are plain names (up to 40
  characters of `A-Z a-z 0-9 . _ -` and space).
- `history` (optional): up to 10 turns, oldest first, `role` `user` | `dagon`,
  `text` 1 to 2000 characters (text only).

200:

```json
{
  "answer": "試す回数を 2 回に減らす案です。\n\n...",
  "proposal": { "yaml_diff": " dagon: 1\n-    max_attempts: 3\n+    max_attempts: 2\n...",
                "topology": "dagon: 1\nname: ...\n" }
}
```

- `answer`: plain text in Japanese. The first sentence answers the question
  (at most 110 characters, for the speech bubble); details, if any, follow after
  a blank line (for the 会話 tab).
- `proposal`: `null`, or a whole new topology that already passed the same
  check as `PUT /v1/topology` (0 errors). `yaml_diff` lists every line of the
  current file and the proposal, one per line, prefixed `' '` (kept), `'-'`
  (removed) or `'+'` (added). Apply it with `PUT /v1/topology` and the
  `topology` text; nothing is changed before that.
- A proposal is whole-file: the current file's comments are not in it. When the
  agent's proposal does not pass the check after 3 calls, the answer comes
  without it and ends with 「提案した構成は確認で誤りが残ったため、出していません。」.

Errors (all `{ "error": "...", "code": "..." }` except the 400s):

| Status | code | When |
|---|---|---|
| 400 | (none) | body is not JSON or a field breaks the rules above |
| 403 | (none) | `dagon demo`, or the Host/Origin check |
| 401 | (none) | no passphrase |
| 409 | `AGENT_NOT_LINKED` | not linked, or the linked agent/way is no longer defined |
| 409 | `AGENT_NOT_LINKED` | also: unlinked while the question was being answered (no further call is made) |
| 409 | `AGENT_UNAVAILABLE` | linked, but the way cannot be used here: its program is not on PATH, or its key variable is not set (`error` names the program or the variable; restart `dagon serve` after setting a key). Replaces round 3's `AGENT_NO_KEY` |
| 429 | `AGENT_BUSY` | another question is being answered (one at a time per serve) |
| 413 | `AGENT_CONTEXT_TOO_LARGE` | the topology file is over 64 KiB (not read), or one prompt would be over 192 KiB (not sent) |
| 504 | `AGENT_TIMEOUT` | one agent call took longer than 120 s (a `command` way's program is stopped) |
| 502 | `AGENT_FAILED` | the agent refused, its program ended with an error, or it answered in an unexpected form; `error` says which (never the program's output) |

What goes to the agent (and only this, the same two texts for every agent
and way): a system text (DAGON's instructions, the topology schema and checks,
the allow-listed facts of the definitions in use, the reply format) and a user
text (the question as one JSON document: the topology as plain data with
comments dropped and every string or key that is not a plain name as
`project-text`, its check results with those words hidden, the selection, the
kind and outcome of the last 5 finished jobs, the history and the question;
the user's words are data in it). Never: keys, file paths, commands, logs, job
errors. A topology with any hidden value gets no file proposal (the answer
then ends with 「この構成には DAGON に送らなかった値があるため、変更の案は出していません。」 and
no repair call is made); a proposal that contains `project-text` is refused
too.

The 192 KiB cap is measured on the system and user texts together before every
call, whatever the way.

## Agent definitions

Builtin: `crates/dagon-cli/agents/claude.toml`. The user may add files in
`~/.dagon/agents/*.toml` (never a project's folder: a way may name a program).
Each file is one agent; unknown keys, a kind DAGON does not implement, bad ids
or an agent id that exists already make the file not load (`errors` above).

```toml
id = "my-agent"          # a-z 0-9 -, up to 32
name = "My agent"
provider = "Me"

[[ways]]
id = "local"
name = "Local program"
[ways.transport]
kind = "command"         # or "messages-http"
program = "my-agent"     # on PATH, or a path
args = ["--answer-only", "--system", "{system_file}"]
system = "file"          # "stdin" (default): system text, a blank line, then the user text on stdin
```

- `command`: runs `program` with `args` in a new empty temporary folder
  (removed afterwards), with the keys' environment variables removed; the user
  text goes to stdin (after the system text when `system = "stdin"`; with
  `system = "file"` the system text is in a file whose path replaces the arg
  `{system_file}`). Stdout is the answer (at most 1 MiB, UTF-8); stderr is never
  read. A program that runs past the call's time is stopped.
- `messages-http`: `url` (https only), `version` (the `anthropic-version`
  header), `key_env` (the variable holding the key), `model`, optional
  `model_env` (a variable that overrides `model`).

The agent replies with one JSON object (the reply format is in the system
text); DAGON reads the object from its text.

## Where the consent lives

`~/.dagon/agent.json` (`{"agent":"claude","way":"claude-code","at":"..."}`), one
per user, shared by `dagon serve` and `dagon ai link` / `dagon ai unlink`. Per
user and not per project: the agent's login or key belongs to the user's
environment, a per-project consent would ask again in every project, and
nothing lands in a project folder that may be committed. A file that does not
parse, holds any other field, has an id that is not plain or an `at` that is
not `YYYY-MM-DDTHH:MM:SSZ` on a real calendar date counts as not linked. A
round 1–4 file (`{"provider":"anthropic",...}`) counts as not linked: the user
links again and chooses the way.

## CLI

- `dagon ai link [--agent <id>] [--way <id>]`: shows the consent text (agent
  and way, where the login or key stays, sends, never sends, how to unlink) and
  records consent on `y`. Default: the first agent's first way (Claude Code).
- `dagon ai unlink`: removes the consent; logins and keys are removed by the
  user on the agent's side.
- `dagon ai topology` refuses until linked.
- `dagon ai ask <question>`: the same question to DAGON about ./dagon.toml's
  topology (no selection, history or job outcomes); prints the answer and, for
  a proposal, its diff. The file is never changed. Refuses until linked.

## Sources for the claude-code way

The flags are those `claude --help` lists in Claude Code 2.1.316 (checked on
2026-11-04): `-p`, `--tools ""` (no built-in tools), `--no-session-persistence`,
`--permission-mode dontAsk`, `--permission-prompts none`, `--safe-mode` (no
CLAUDE.md, skills, plugins, hooks, MCP servers, custom agents),
`--strict-mcp-config`, `--disable-slash-commands`, `--output-format text`,
`--system-prompt-file`. `--bare` is not used: it reads only `ANTHROPIC_API_KEY`,
not the user's login. The set was run once by hand with a one-line prompt
(answer printed on stdout).
