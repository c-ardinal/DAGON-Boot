---
title: "プロジェクトでのClaudeの手助け"
description: "新しいプロジェクトに入るClaude Codeの指示とスキル、DAGONの移植に使うClaude Codeのプラグインです。"
---

`dagon new`は、プロジェクトにClaude Codeのファイルも書きます。プロジェクトのフォルダで開いたClaude Codeは、構成のファイル、コマンド、守ることを知った状態で始まります。

| ファイル | 役目 |
|---|---|
| `CLAUDE.md` | プロジェクトの配置、自分のファイルと生成されるファイルの区別、コマンド、守ること（鍵に触れない、ハードウェアの値は推測せず質問する） |
| `.claude/skills/fix-a-failing-build/SKILL.md` | `dagon build`が失敗したとき: 原因を探し、`nodes/<id>/src`の自分のコードだけを直して、もう一度ビルドする |
| `.claude/skills/add-a-board-by-mcu/SKILL.md` | 一覧に無いボードをマイコンで選んで加える。値はボードの資料から出典付きで |
| `.claude/skills/explain-a-topology-problem/SKILL.md` | 構成の検査が示す問題を分かる言葉で説明し、いちばん小さい直し方を示す |

これらのファイルには、鍵も、このPCのパスも入りません。

署名の鍵とClaudeのAPIキーは`dagon`の中にとどまります。Claudeにはそれらを開かないよう指示してあります。署名と実機への書き込みは開発者が行います。

## 新しいマイコンへの移植

このリポジトリの`plugin/`は、DAGONを新しいマイコンへ移植するためのClaude Codeのプラグイン（`/dagon-port`）です。移植は適合試験（`dagon conform`）で確かめます。
