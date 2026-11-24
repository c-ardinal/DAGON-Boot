---
title: "独自のボード（マイコンで選択）"
description: "一覧にないボードを、そのボードの定義の記述によって使用します。"
---

[ボードとマイコン](/reference/boards/)の一覧にあるボードは、選択だけで使用可能です。ピン、既定のリンク、付属の部品は、ボードの定義に記述済です。

一覧にないボードは、搭載するマイコンを選択し、ボードの定義を利用者が記述します。

ボードの定義は`dagon.toml`に記述します。表の名称`[board.<ボードのid>]`がボードのidです。欄は[マイコンとボードの定義](/reference/definitions/)にあるボードの定義と同一で、`id`だけは記述しません。

```toml
[board.my-f401]
format = 4
product = "My F401 board"
mcu = "stm32f401re"
flash_bytes = 524288
i2c_target = true
drawing = "nucleo-64"
i2c = [{ controller = "i2c3", sda = "PB4", scl = "PA8" }]
```

`topology.yaml`にある該当ノードの`board:`に、そのボードのidを記述します。

```yaml
  - id: seg
    mcu: stm32f401re
    board: my-f401
    role: node
    port: self-boot
    flash_map: f401-default
    tag: seg-f401-7seg
```

`dagon gen`はこのノードをマイコンのcrateから作成し、`wiring.rs`にボードの`flash_bytes`を出力します。

接続先のノード側の端には、そのノードのマイコン定義にある`[[link_port]]`の名称（`port`）を記述します。I2Cで接続する接続先のノードでは、その端にアドレス（`addr`）とピン（`node_pins`）も記述します（[構成のリンク](/reference/topology/)）。

不足する項目は検査と`dagon gen`が通知します。

Claude Codeと共同で作業する場合は`add-a-board-by-mcu`スキルを使用します。

## 一覧のボードとの部分的な相違

一覧のボードと一部だけ異なるボード（ピンの割当が異なる版など）は、ボードの定義を複製せずに、相違する欄だけを`dagon.toml`の`[board_override.<ボードのid>]`に記述します。記述しない欄は一覧のボードの定義のままです。上書きはプロジェクト全体に適用し、そのボードを使用する全ノードが対象です。一部のノードだけが異なる場合は、`[board.<ボードのid>]`で新規idのボードを定義します。

例は表示装置とタッチコントローラのリセットピンを交換した版のWaveshare RP2040 Touch LCD 1.28です。`[[board_override."waveshare-rp2040-touch-lcd-1.28".pin]]`のエントリを2つ記述します。1つは`pin = "GPIO22"`と`name = "lcd_rst"`、もう1つは`pin = "GPIO13"`と`name = "touch_rst"`です。`[[pin]]`のエントリは`pin`で検索し、記述した`name`だけを変更します。

`dagon gen`は上書きを適用したボードの定義を、ノードごとに`boot/<id>/board.toml`に出力します。ボードのcrateとブートローダは、そのファイルからビルドします。上書きの変更後は、`dagon gen`を再度実行します。マイコンの定義も同様に`[mcu_override.<マイコンのid>]`で上書き可能です。合成の規則、削除（`dagon-remove`）、欄の解除（`dagon-unset`）、順序（`dagon-order`）、変更不可の欄は、[マイコンとボードの定義](/reference/definitions/)の「プロジェクトの定義」にあります。
