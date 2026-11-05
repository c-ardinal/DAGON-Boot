---
title: "Agent link: serve API"
description: "How DAGON reaches its agent through dagon serve: routes, consent, keys and what is sent."
---

DAGON talks to an agent through one interface. Agents are data: an agent
(id, name, provider) and the ways to reach it, each of a transport kind DAGON
implements (see Builtin agents and Transport kinds below). The user consents once, when linking,
to one agent and one way; after that questions are sent without asking again.
Logins stay with the agent; API keys stay in the OS credential store (see
Where API keys live). Neither ever reaches the page.

All routes below are under `dagon serve` with the same guards as every `/v1/`
route: Host and Origin checks (403), `Authorization: Bearer <passphrase>` (401).

## GET /v1/agent

```json
{
  "agents": [
    { "id": "claude", "name": "Claude", "provider": "Anthropic",
      "ways": [
        { "id": "claude-code", "name": "Claude Code", "kind": "command", "available": true, "key": null },
        { "id": "api", "name": "API キー", "kind": "messages-http", "available": false, "key": false }
      ] }
  ],
  "default": { "agent": "claude", "way": "claude-code" },
  "linked": false,
  "consent": null,
  "available": true,
  "provider": { "id": "claude", "name": "Claude (Anthropic)" },
  "errors": [],
  "consent_error": null,
  "sends": ["…"],
  "never_sends": ["…"]
}
```

- `agents`: builtin first, then the user's own (`~/.dagon/agents/*.toml`).
  Each way has `kind` (`command` | `messages-http`; more kinds may come, so
  treat an unknown one as opaque) and `available`: whether it
  can be used on this machine now (a `command` way: its program is on PATH; a
  `messages-http` way: its key is registered). `key`: for a way that takes
  an API key, whether one is registered (`true` / `false`); `null` for a way
  that takes none. No program, argument, URL or key is ever in this body.
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
- `consent_error`: when ~/.dagon/agent.json is of a version this dagon does
  not read, why (link again to record one it reads); else `null`.
- `sends` / `never_sends`: the lists under What linking sends, which the
  linking dialog shows. The terminal (`dagon ai link`) shows the same lists.

## POST /v1/agent/link

Body: `{ "agent": "claude", "way": "claude-code", "consent": true }`. Records
the consent for that agent and way (a new link replaces the old one; changing
the way asks for consent again).

- 200: the same body as GET /v1/agent (now `linked: true`).
- 400 `agent and way must name a defined agent and way` /
  `consent must be true` (only the JSON boolean `true` counts); nothing is
  recorded.
- 403 in `dagon demo` (linking is refused there; GET still works).
- 500 `the consent could not be saved`.

Linking does not need the way to be available (the user may install Claude
Code or register the key afterwards).

## DELETE /v1/agent/link

Withdraws the consent (also fine when none was recorded).

- 200: the same body as GET /v1/agent (now `linked: false`).
- 403 in `dagon demo`.
- **Unlinking waits for an agent call in flight.** Every call holds the
  consent from its check until the call ends (a shared lock on
  `~/.dagon/agent.lock`, one handle per call, so threads and processes such as
  `dagon serve` and `dagon ai unlink` exclude each other alike). DELETE
  removes the consent at once (a call reads it again under the lock, so no
  new call passes from then on), then takes the lock exclusively, so it
  answers 200 only when no call is in flight. The wait is at most one call:
  up to about 120 s for a question, up to 600 s for a round of a topology
  draft. The page should show a waiting state until DELETE answers.
- No call passes the check after that: running `ai_topology` jobs are
  cancelled, a question being answered makes no further call and answers 409
  `AGENT_NOT_LINKED` instead of its answer (`dagon ai ask` does not print it
  either).

## PUT /v1/agent/key/{agent}/{way}

Body: `{ "key": "..." }`. Registers the way's API key in the OS credential
store (replacing the one before). The key is taken without surrounding
blanks; it must be 1 to 1024 visible ASCII characters (it goes into an HTTP
header; Windows Credential Manager holds at most 2560 bytes, stored as
UTF-16). The key is never answered, logged or written to a file; the page
keeps it in no storage and clears its field once sent.

- 200: the same body as GET /v1/agent (the way's `key` is now `true`).
- 400 `this way takes no key` (a login way), or `key must be a string`, or
  why the key is refused; nothing is stored.
- 404 `no such agent and way`.
- 403 in `dagon demo`, or the Host/Origin check (another origin is refused).
- 503 `AGENT_KEY_STORE`: the OS credential store cannot be used here (for
  example Linux without a running Secret Service); `error` says why and how
  to install or start one. Nothing is stored anywhere else.

## DELETE /v1/agent/key/{agent}/{way}

Removes the way's key from the store (also fine when none was registered).

- 200: the same body as GET /v1/agent (the way's `key` is now `false`).
- 400 / 404 / 403 / 503 as for PUT.

## Where API keys live

Only in the OS credential store: Windows Credential Manager, the macOS
Keychain, or the Secret Service (GNOME Keyring, KeePassXC, ...) on Linux.
Each key is the entry of service `dagon`, account `<agent id>/<way id>`
(`claude/api`). DAGON reads no key from an environment variable and keeps no
key in a file; where no store can be used, registering is refused. A key
leaves the store only into the request header of its way.

## AI routes before linking

Every `/v1/ai/...` route answers, until linked:

```json nocheck
HTTP 409
{ "error": "DAGON is not linked to an agent; link it first (consent is asked once)",
  "code": "AGENT_NOT_LINKED" }
```

Match on `code`, not on the text. A consent file of a version this dagon does
not read answers 409 `AGENT_CONSENT_VERSION` instead, on every route above. Once linked the routes behave as before
(`/v1/ai/topology` starts a job; a way that cannot be used here fails inside
the job with its reason, e.g. `no API key is registered for claude/api`).

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
| 409 | `AGENT_CONSENT_VERSION` | ~/.dagon/agent.json is of a version this dagon does not read; `error` says which and to link again |
| 409 | `AGENT_UNAVAILABLE` | linked, but the way cannot be used here: its program is not on PATH, or no key is registered for it (`error` names the program, or the way and how to register its key). |
| 429 | `AGENT_BUSY` | another question is being answered (one at a time per serve) |
| 413 | `AGENT_CONTEXT_TOO_LARGE` | the topology file is over 64 KiB (not read), or one prompt would be over 192 KiB (not sent) |
| 504 | `AGENT_TIMEOUT` | one agent call took longer than 120 s (a `command` way's program is stopped) |
| 502 | `AGENT_FAILED` | the agent refused, its program ended with an error, or it answered in an unexpected form; `error` says which (never the program's output; an error from the API is cut to 300 characters, and the key never appears in an answer or an error, even when the API echoes it) |

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

**A command agent runs with your permissions; only add programs you trust.**
DAGON does not sandbox the program a way names and does not claim to. What
DAGON controls is what it sends (the prompt below) and what the program
inherits (the environment allow-list, a fresh working folder, the time and
output limits). Everything else the program does is up to the program: the
builtin Claude Code way relies on Claude Code's own flags to turn off its
tools, MCP servers, customisations and saved sessions.

```toml
dagon_agent = 1          # the definition format; another number is refused with the reason
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
pass_env = ["MY_AGENT_HOME"]  # optional: more variables the program needs
```

Every key, its type and whether it is required are in
[Agent definition format](/reference/agent-definition/), generated from the
types dagon reads the file into. Each `kind` is one of the Transport kinds below.

The agent replies with one JSON object (the reply format is in the system
text); DAGON reads the object from its text.

## Transport kinds

- `command`: runs `program` with `args`; the user text goes to stdin (after
  the system text when `system = "stdin"`; with `system = "file"` the system
  text is in a file whose path replaces the arg `{system_file}`). Stdout is the
  answer (at most 1 MiB, UTF-8); stderr is never read.
  - Environment: only `PATH`, `PATHEXT`, `HOME`, `USERPROFILE`, `APPDATA`,
    `LOCALAPPDATA`, `TEMP`, `TMP`, `TMPDIR`, `SYSTEMROOT`, `WINDIR`, `COMSPEC`,
    `LANG`, `LC_*`, `TERM` (those that are set), plus the names in `pass_env`.
    Never `DAGON_*`, the signing key's variable or `ANTHROPIC_API_KEY`, even
    when listed.
  - Working folder: a fresh temporary folder, removed afterwards, so the
    program does not start in the project. This is hygiene, not a sandbox:
    the program can still reach whatever your account can.
  - Lifetime: the program runs in an OS group: a Windows job object (through
    the `command-group` crate; closing it kills every process in it, also
    when dagon itself ends) or a Unix process group it leads. The group is
    stopped when the call's time runs out and as soon as the program itself
    ends, so a process it started and left behind (holding its stdout or
    not) never outlives the call. The answer is what the program printed
    before it ended.
- `messages-http`: `url` (https only), `version` (the `anthropic-version`
  header), `model`, optional `model_env` (a variable that overrides `model`).
  The key is the way's entry in the OS credential store (Where API keys
  live); a definition naming a key variable (`key_env`) is refused. The
  answer is read up to 1 MiB; a larger one is refused while it is read.

## Where the consent lives

`~/.dagon/agent.json` (`{"version":1,"agent":"claude","way":"claude-code","at":"..."}`), one
per user, shared by `dagon serve` and `dagon ai link` / `dagon ai unlink`. Per
user and not per project: the agent's login or key belongs to the user,
a per-project consent would ask again in every project, and
nothing lands in a project folder that may be committed. A file that does not
parse, has another `version`, holds any other field, has an id that is not
plain or an `at` that is not `YYYY-MM-DDTHH:MM:SSZ` on a real calendar date
counts as not linked: the user links again.

## Builtin agents

The agents shipped with dagon (`crates/dagon-cli/agents/*.toml`) and their
ways, in order, are listed in [Agent definition format](/reference/agent-definition/#builtin-agents);
the first way of the first agent is the default.

## What linking sends

### Sends

- 構成と定義の事実（ノード・リンク・部品・設定値）
- 選んだノードやリンク
- 確かめた結果（問題・◇ 未対応・更新の記録）

### Never sends

- 鍵
- コマンドとその出力の全文
- ファイルのパス

## Format numbers

Each file format has a number; a reader refuses one it does not know and
says which number it reads.

- `dagon_agent = 1`: an agent definition file
- `"version": 1`: the consent file, ~/.dagon/agent.json

## CLI

The `dagon ai` commands (`link`, `unlink`, `key`, `ask`, `topology`) and their
options are in [Command line](/reference/cli/#dagon-ai). Linking shows the
consent text (agent and way, where the login or key stays, sends, never
sends, how to unlink) and records consent on `y`; every AI command refuses
until DAGON is linked, and `dagon ai ask` never changes the topology file.

## Checking a command way against the installed program

Each builtin way's flags, and why each is there, are in its definition
(`crates/dagon-cli/agents/*.toml`), not repeated here. The claude-code way can
be run through DAGON's own spawn path (the environment allow-list, a fresh
working folder, the system file) with
`cargo test -p dagon-cli claude_code_answers -- --ignored` (ignored by default:
it uses the network and the user's login; one line is sent, no project data).
