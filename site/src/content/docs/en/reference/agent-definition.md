---
title: Agent definition format
description: The keys of an agent definition file, from the types dagon reads it into.
editUrl: false
---

:::note
Generated from `crates/dagon-cli/src/agents/`, `crates/dagon-cli/agents/`; do not edit. Regenerate with `python site/scripts/gen.py`.
:::

An agent is a TOML file: the builtin ones ship with `dagon`, and you add your
own in `~/.dagon/agents/*.toml` (never in a project). Unknown keys, a kind
DAGON does not implement or an id that is taken make the file not load.

Definition format: **1** (`dagon_agent = 1`). A dagon that reads another
number refuses the file and says why.

## Agent

| Key | Type | Required | Meaning |
|---|---|---|---|
| `dagon_agent` | integer | yes | The definition format, `FORMAT`. |
| `id` | string | yes | The agent's id: lower-case letters, digits and '-', up to 32; unique among all agents. |
| `name` | string | yes | The name the linking texts show. |
| `provider` | string | yes | Who makes the agent, shown after its name. |
| `ways` | `[[ways]]` tables | yes | The ways to reach the agent, in order; the first is the default. |

## `[[ways]]`

| Key | Type | Required | Meaning |
|---|---|---|---|
| `id` | string | yes | The way's id, unique within its agent (same characters as an agent id). |
| `name` | string | yes | The name the linking texts show for this way. |
| `transport` | `[ways.transport]` table | yes | How DAGON reaches the agent this way: one of the transport kinds and its keys. |

## `[ways.transport]`

`kind` is one of: `command`, `messages-http`.

### `kind = "command"`

| Key | Type | Required | Meaning |
|---|---|---|---|
| `program` | string | yes | A name looked up on PATH, or a path. |
| `args` | array of strings | no | The arguments, in order; `{system_file}` is replaced by the system text's file when `system = "file"`. |
| `system` | string | no | How the system text reaches the program. |
| `pass_env` | array of strings | no | Environment variables passed on besides `BASE_ENV` (names only). |

`system` is one of: `stdin` (Before the user text on stdin.), `file` (In a file in the call's folder, named by the `{system_file}` arg.).

### `kind = "messages-http"`

| Key | Type | Required | Meaning |
|---|---|---|---|
| `url` | string | yes | The messages endpoint, https only. |
| `version` | string | yes | The `anthropic-version` header. |
| `model` | string | yes | The model named in each request. |
| `model_env` | string | no | An environment variable that overrides `model`, if set. |

## Builtin agents

| Id | Name | Provider | Ways |
|---|---|---|---|
| `claude` | Claude | Anthropic | `claude-code` (command), `api` (messages-http) |
