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
| `id` | string | yes |  |
| `name` | string | yes |  |
| `provider` | string | yes |  |
| `ways` | `[[ways]]` tables | yes |  |

## `[[ways]]`

| Key | Type | Required | Meaning |
|---|---|---|---|
| `id` | string | yes |  |
| `name` | string | yes |  |
| `transport` | `[ways.transport]` table | yes |  |

## `[ways.transport]`

`kind` is one of: `command`, `messages-http`.

### `kind = "command"`

| Key | Type | Required | Meaning |
|---|---|---|---|
| `program` | string | yes | A name looked up on PATH, or a path. |
| `args` | array of strings | no |  |
| `system` | string | no | How the system text reaches the program. |
| `pass_env` | array of strings | no | Environment variables passed on besides `BASE_ENV` (names only). |

`system` is one of: `stdin` (Before the user text on stdin.), `file` (In a file in the call's folder, named by the `{system_file}` arg.).

### `kind = "messages-http"`

| Key | Type | Required | Meaning |
|---|---|---|---|
| `url` | string | yes | The messages endpoint, https only. |
| `version` | string | yes | The `anthropic-version` header. |
| `key_env` | string | yes | The environment variable holding the key. |
| `model` | string | yes |  |
| `model_env` | string | no | An environment variable that overrides `model`, if set. |

## Builtin agents

| Id | Name | Provider | Ways |
|---|---|---|---|
| `claude` | Claude | Anthropic | `claude-code` (command), `api` (messages-http) |
