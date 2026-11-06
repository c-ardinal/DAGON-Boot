---
title: "自分のボード（マイコンで選ぶ）"
description: "一覧に無いボードを、マイコンで選んで使います。"
---

[ボードとマイコン](/reference/boards/)の一覧にあるボードは、選ぶだけで使えます。ピン、既定のリンク、付属の部品は、ボードの定義が持っています。

一覧に無いボードは、載っているマイコンで選びます。フラッシュ容量と、親機につないだピンは自分で書きます。

`topology.yaml`のそのノードから`board:`を外します。

```yaml
  - id: seg
    mcu: stm32f401re
    role: node
    port: self-boot
    flash_map: f401-default
    tag: seg-f401-7seg
```

ボードの値を`dagon.toml`に書きます。

```toml
[board.seg]
flash_bytes = 524288
```

I2Cの子機では、リンクにノード側のピン（`node_pins`）を書きます。

足りないものは、検査と`dagon gen`が知らせます。

Claude Codeと一緒に進めるときは`add-a-board-by-mcu`スキルを使います。
