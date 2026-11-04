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

`GET /v1/hello` keeps `api_key` (same value as `available`).

## Where the consent lives

`~/.dagon/agent.json` (`{"provider":"anthropic","at":"..."}`), one per user,
shared by `dagon serve` and `dagon ai link` / `dagon ai unlink`. Per user and
not per project: the key it goes with belongs to the user's environment, a
per-project consent would ask again in every project, and nothing lands in a
project folder that may be committed. A file that does not parse or names
another provider counts as not linked.

## CLI

- `dagon ai link`: shows the consent text (provider, key location, sends,
  never sends, how to unlink) and records consent on `y`.
- `dagon ai unlink`: removes the consent; the key is removed by the user from
  the environment.
- `dagon ai topology` refuses until linked.
