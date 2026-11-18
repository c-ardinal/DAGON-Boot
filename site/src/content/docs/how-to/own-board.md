---
title: "自分のボード（マイコンで選ぶ）"
description: "一覧に無いボードを、そのボードの定義を書いて使います。"
---

[ボードとマイコン](/reference/boards/)の一覧にあるボードは、選ぶだけで使えます。ピン、既定のリンク、付属の部品は、ボードの定義が持っています。

一覧に無いボードは、載っているマイコンを選び、ボードの定義を自分で書きます。

ボードの定義を`dagon.toml`に書きます。表の名前`[board.<ボードのid>]`がボードのidです。欄は[マイコンとボードの定義](/reference/definitions/)のボードの定義と同じで、`id`だけを書きません。

```toml
[board.my-f401]
format = 3
product = "My F401 board"
mcu = "stm32f401re"
flash_bytes = 524288
i2c_target = true
drawing = "nucleo-64"
i2c = [{ controller = "i2c3", sda = "PB4", scl = "PA8" }]
```

`topology.yaml`のそのノードの`board:`に、そのボードのidを書きます。

```yaml
  - id: seg
    mcu: stm32f401re
    board: my-f401
    role: node
    port: self-boot
    flash_map: f401-default
    tag: seg-f401-7seg
```

`dagon gen`は、このノードをマイコンのcrateから作成し、`wiring.rs`にボードの`flash_bytes`を書きます。

接続先のノード側の端には、そのノードのマイコンの`[[link_port]]`の名前（`port`）を書きます。I2Cで接続する接続先のノードでは、その端にアドレス（`addr`）とピン（`node_pins`）も書きます（[構成のリンク](/reference/topology/)）。

足りないものは、検査と`dagon gen`が知らせます。

Claude Codeと一緒に進めるときは`add-a-board-by-mcu`スキルを使います。

## 一覧にあるボードと一部だけ違うボード

一覧にあるボードと一部だけ違うボード（ピンの割り当てが違う版など）は、ボードの定義を写さずに、違う欄だけを`dagon.toml`の`[board_override.<ボードのid>]`に書きます。書かない欄は一覧のボードの定義のままです。上書きはプロジェクト全体のもので、そのボードを使うすべてのノードに当たります。一部のノードだけが違うときは、`[board.<ボードのid>]`で新しいidのボードを定義します。

例: 表示装置とタッチのコントローラのリセットのピンが入れ替わった版の、Waveshare RP2040 Touch LCD 1.28です。`[[board_override."waveshare-rp2040-touch-lcd-1.28".pin]]`のエントリを2つ書きます。1つは`pin = "GPIO22"`と`name = "lcd_rst"`、もう1つは`pin = "GPIO13"`と`name = "touch_rst"`です。`[[pin]]`のエントリは`pin`で見つけ、書いた`name`だけを変えます。

`dagon gen`は、上書きを当てたボードの定義を、ノードごとに`boot/<id>/board.toml`に書きます。ボードのcrateとブートローダは、そのファイルからビルドします。上書きを変えたら、`dagon gen`をもう一度実行します。マイコンの定義も同じように`[mcu_override.<マイコンのid>]`で上書きできます。合成の規則、消すこと（`dagon-remove`）、欄を外すこと（`dagon-unset`）、順（`dagon-order`）、変えられない欄は、[マイコンとボードの定義](/reference/definitions/)の「プロジェクトの定義」にあります。
