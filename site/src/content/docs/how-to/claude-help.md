---
title: "プロジェクトでのClaudeの支援"
description: "新規プロジェクトに付属するClaude Codeの指示とスキル、DAGONの移植に使用するClaude Codeのプラグインです。"
---

`dagon new`は、1つのエージェントの[定義](/reference/agent-definition/)にあるファイルもプロジェクトに出力します。既定は組込のClaudeで、出力するのは以下のClaude Codeのファイルです。`--agent <id>`の場合は指定したエージェントのファイルを出力し、`--no-agent`の場合は出力しません。プロジェクトのフォルダで起動したClaude Codeは、構成のファイル、コマンド、遵守事項を把握した状態で開始します。

| ファイル | 役割 |
|---|---|
| `CLAUDE.md` | プロジェクトの配置、利用者のファイルと生成ファイルの区別、コマンド、遵守事項（キーへのアクセス禁止、ハードウェアの値は推測せずに質問） |
| `.claude/skills/fix-a-failing-build/SKILL.md` | `dagon build`の失敗時: 原因を調査し、`nodes/<id>/src`の利用者のコードだけを修正して再度ビルド |
| `.claude/skills/add-a-board-by-mcu/SKILL.md` | 一覧にないボードをマイコンで選択して追加。値はボードの資料から出典付きで記述 |
| `.claude/skills/explain-a-topology-problem/SKILL.md` | 構成の検査で判明した問題を平易な表現で説明し、最小の修正方法を提示 |

これらのファイルには、キーとこのPCのパスの記載はありません。

署名のキーとClaudeのAPIキーは`dagon`の内部だけで管理します。Claudeにはこれらへのアクセスを禁止する指示があります。署名と実機への書込は開発者が実行します。

## 新規マイコンへの移植

このリポジトリの`plugin/`は、DAGONを新規マイコンへ移植するためのClaude Codeのプラグイン（`/dagon-port`）です。移植は適合試験（`dagon conform`）で検証します。
