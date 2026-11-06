---
title: エージェント定義の形式
description: エージェントの定義ファイルのキーです。
editUrl: false
---

:::note
`crates/dagon-cli/src/agents/`, `crates/dagon-cli/agents/` から生成しています。直接編集せず、`python site/scripts/gen.py` でもう一度生成してください。
「意味」の列は元のソースのままで、英語です。
:::

エージェントは TOML ファイルです。組み込みのものは `dagon` に入っていて、自分のものは
`~/.dagon/agents/*.toml` に足します（プロジェクトの中には置きません）。知らないキー、
DAGON に無い種類、使われている id があるファイルは読み込みません。

定義の形式: **1**（`dagon_agent = 1`）。別の番号のファイルは、理由を示して受け付けません。

## エージェント

| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `dagon_agent` | 整数 | 必須 | The definition format, `FORMAT`. |
| `id` | 文字列 | 必須 | The agent's id: lower-case letters, digits and '-', up to 32; unique among all agents. |
| `name` | 文字列 | 必須 | The name the linking texts show. |
| `provider` | 文字列 | 必須 | Who makes the agent, shown after its name. |
| `ways` | `[[ways]]` テーブル | 必須 | The ways to reach the agent, in order; the first is the default. |

## `[[ways]]`

| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `id` | 文字列 | 必須 | The way's id, unique within its agent (same characters as an agent id). |
| `name` | 文字列 | 必須 | The name the linking texts show for this way. |
| `transport` | `[ways.transport]` テーブル | 必須 | How DAGON reaches the agent this way: one of the transport kinds and its keys. |

## `[ways.transport]`

`kind` は次のどれかです: `command`, `messages-http`。

### `kind = "command"`

| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `program` | 文字列 | 必須 | A name looked up on PATH, or a path. |
| `args` | 文字列の配列 | 任意 | The arguments, in order; `{system_file}` is replaced by the system text's file when `system = "file"`. |
| `system` | 文字列 | 任意 | How the system text reaches the program. |
| `pass_env` | 文字列の配列 | 任意 | Environment variables passed on besides `BASE_ENV` (names only). |

`system` は次のどれかです: `stdin` (Before the user text on stdin.), `file` (In a file in the call's folder, named by the `{system_file}` arg.)。

### `kind = "messages-http"`

| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `url` | 文字列 | 必須 | The messages endpoint, https only. |
| `version` | 文字列 | 必須 | The `anthropic-version` header. |
| `model` | 文字列 | 必須 | The model named in each request. |
| `model_env` | 文字列 | 任意 | An environment variable that overrides `model`, if set. |

## 組み込みのエージェント

| Id | 名前 | 提供元 | つなぎ方 |
|---|---|---|---|
| `claude` | Claude | Anthropic | `claude-code` (command), `api` (messages-http) |
