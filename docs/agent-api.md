# DAGON agent linking: serve API contract (UX3C)

DAGON talks to one agent, Claude (Anthropic). The user consents once, when
linking; after that questions are sent without asking again. The API key stays
in the CLI's environment (`ANTHROPIC_API_KEY`) and never reaches the page.

All routes below are under `dagon serve` with the same guards as every `/v1/`
route: Host and Origin checks (403), `Authorization: Bearer <passphrase>` (401).

## GET /v1/agent

```json
{
  "available": true,
  "linked": false,
  "provider": { "id": "anthropic", "name": "Claude (Anthropic)" },
  "consent": null,
  "sends": [
    "構成と定義の事実（ノード・リンク・部品・設定値）",
    "選んだノードやリンク",
    "確かめた結果（問題・◇ 未対応・更新の記録）"
  ],
  "never_sends": ["鍵", "コマンドとその出力の全文", "ファイルのパス"]
}
```

- `available`: this CLI has `ANTHROPIC_API_KEY` set (the key itself is never sent).
- `linked`: consent is recorded. Independent of `available`: a user may link
  first and set the key afterwards (restart `dagon serve` after setting it).
  DAGON can answer only when both are true.
- `consent`: `{ "at": "2026-11-04T04:14:35Z", "provider": "anthropic" }` once
  linked (`at` is UTC, RFC 3339), else `null`.
- `sends` / `never_sends`: the lists the linking dialog shows (from the round8c
  string table). The terminal (`dagon ai link`) shows the same lists.

## POST /v1/agent/link

Body: `{ "provider": "anthropic", "consent": true }`. Records the consent.

- 200: the same body as GET /v1/agent (now `linked: true`).
- 400 `provider must be anthropic` / `consent must be true` (only the JSON
  boolean `true` counts); nothing is recorded.
- 403 in `dagon demo` (linking is refused there; GET still works).
- 500 `the consent could not be saved`.

## DELETE /v1/agent/link

Withdraws the consent (also fine when none was recorded).

- 200: the same body as GET /v1/agent (now `linked: false`).
- 403 in `dagon demo`.
- **Unlinking waits for a model call in flight.** Every model call holds the
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
(`/v1/ai/topology` starts a job; a missing key fails inside the job with the
`ANTHROPIC_API_KEY is not set` message).

In `dagon demo` every route that would reach an agent answers 403
(`/v1/agent/link` POST/DELETE, `/v1/agent/ask`, `/v1/ai/...`): a demo never
calls a real agent. `GET /v1/agent` still answers, and `ai_topology` is left out
of the demo's `features`.

`GET /v1/hello` keeps `api_key` (same value as `available`).

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
  model's proposal does not pass the check after 3 calls, the answer comes
  without it and ends with 「提案した構成は確認で誤りが残ったため、出していません。」.

Errors (all `{ "error": "...", "code": "..." }` except the 400s):

| Status | code | When |
|---|---|---|
| 400 | (none) | body is not JSON or a field breaks the rules above |
| 403 | (none) | `dagon demo`, or the Host/Origin check |
| 401 | (none) | no passphrase |
| 409 | `AGENT_NOT_LINKED` | not linked |
| 409 | `AGENT_NO_KEY` | linked, but this CLI has no `ANTHROPIC_API_KEY` (restart `dagon serve` after setting it) |
| 429 | `AGENT_BUSY` | another question is being answered (one at a time per serve) |
| 409 | `AGENT_NOT_LINKED` | also: unlinked while the question was being answered (no further call is made) |
| 413 | `AGENT_CONTEXT_TOO_LARGE` | the topology file is over 64 KiB (not read), or one request to the agent would be over 192 KiB (not sent) |
| 504 | `AGENT_TIMEOUT` | one model call took longer than 120 s |
| 502 | `AGENT_FAILED` | the agent refused or answered in an unexpected form; `error` says which |

What goes to the agent (and only this): the allow-listed facts of the
definitions in use (the same board table as `dagon ai topology`), the
topology as plain data (comments dropped; every string or key that is not a
plain name becomes `project-text`), its check results with those words hidden
(a file that does not parse is sent only as "does not parse"), the selection,
the kind and outcome (`build` / `failed` ...) of the last 5 finished jobs, the
history and the question, all as one JSON document (the user's words are
data in it, not text between delimiters). Never: keys, file paths, commands,
logs, job errors. A topology with any hidden value gets no file proposal: the
answer then ends with 「この構成には DAGON に送らなかった値があるため、変更の案は出していません。」
and no repair call is made; a proposal that contains `project-text` is refused too.

The 192 KiB cap is measured on the whole request body (instructions,
definition facts, topology, check, history, question, and the repair rounds)
before every call. The longest question and history with the builtin
definitions and a desk-sized topology stay under it (tested); a topology near
64 KiB or large project definitions can go over it and get
`AGENT_CONTEXT_TOO_LARGE` without any call.

## Where the consent lives

`~/.dagon/agent.json` (`{"provider":"anthropic","at":"..."}`), one per user,
shared by `dagon serve` and `dagon ai link` / `dagon ai unlink`. Per user and
not per project: the key it goes with belongs to the user's environment, a
per-project consent would ask again in every project, and nothing lands in a
project folder that may be committed. A file that does not parse, holds any
field besides those two, names another provider or has an `at` that is not
`YYYY-MM-DDTHH:MM:SSZ` on a real calendar date counts as not linked.

## CLI

- `dagon ai link`: shows the consent text (provider, key location, sends,
  never sends, how to unlink) and records consent on `y`.
- `dagon ai unlink`: removes the consent; the key is removed by the user from
  the environment.
- `dagon ai topology` refuses until linked.
- `dagon ai ask <question>`: the same question to DAGON about ./dagon.toml's
  topology (no selection, history or job outcomes); prints the answer and, for
  a proposal, its diff. The file is never changed. Refuses until linked.
