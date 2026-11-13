---
title: "自分のボード（マイコンで選ぶ）"
description: "一覧に無いボードを、そのボードの定義を書いて使います。"
---

[ボードとマイコン](/reference/boards/)の一覧にあるボードは、選ぶだけで使えます。ピン、既定のリンク、付属の部品は、ボードの定義が持っています。

一覧に無いボードは、載っているマイコンを選び、ボードの定義を自分で書きます。

ボードの定義を`dagon.toml`に書きます。表の名前`[board.<ボードのid>]`がボードのidです。欄は[マイコンとボードの定義](/reference/definitions/)のボードの定義と同じで、`id`だけを書きません。

```toml
[board.my-f401]
format = 2
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

`dagon gen`は、このノードをマイコンのcrateから作り、`wiring.rs`にボードの`flash_bytes`を書きます。

リンクの子機の端には、その子機のマイコンの`[[link_port]]`の名前（`port`）を書きます。I2Cの子機では、その端にアドレス（`addr`）とピン（`node_pins`）も書きます（[構成のリンク](/reference/topology/)）。

足りないものは、検査と`dagon gen`が知らせます。

Claude Codeと一緒に進めるときは`add-a-board-by-mcu`スキルを使います。
