---
title: "プロジェクトでの Claude の手助け"
description: "新しいプロジェクトに入る Claude Code の指示とスキル、DAGON の移植に使うプラグインです。"
---

`dagon new` は、プロジェクトに Claude Code のファイルも書きます。プロジェクトのフォルダで開いた Claude Code は、構成のファイル、コマンド、守ることを知った状態で始まります。

| ファイル | 役目 |
|---|---|
| `CLAUDE.md` | プロジェクトの配置、自分のファイルと生成されるファイルの区別、コマンド、守ること（鍵に触れない、ハードウェアの値は推測せず質問する） |
| `.claude/skills/fix-a-failing-build/SKILL.md` | `dagon build` が失敗したとき: 原因を探し、`nodes/<id>/src` の自分のコードだけを直して、もう一度ビルドする |
| `.claude/skills/add-a-board-by-mcu/SKILL.md` | 一覧に無いボードをマイコンで選んで加える。値はボードの資料から出典付きで |
| `.claude/skills/explain-a-topology-problem/SKILL.md` | 構成の検査が示す問題を分かる言葉で説明し、いちばん小さい直し方を示す |

これらのファイルには、鍵も、この PC のパスも入りません。

署名の鍵と Claude の API キーは `dagon` の中にとどまります。Claude にはそれらを開かないよう指示してあります。署名と実機への書き込みは開発者が行います。

## 新しいマイコンへの移植

このリポジトリの `plugin/` は、DAGON を新しいマイコンへ移植するための Claude Code のプラグイン（`/dagon-port`）です。移植は適合試験（`dagon conform`）で確かめます。
