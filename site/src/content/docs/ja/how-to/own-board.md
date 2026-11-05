---
title: "自分のボード（マイコンで選ぶ）"
description: "一覧に無いボードを、マイコンで選んで使います。"
---

**ボードでもマイコンでも選べます。** 対応表のボードは、ピン・既定のリンク・付属の部品を持っているので、選ぶだけです。同じマイコンを載せたほかのボードはマイコンで選び、フラッシュ容量と親機につないだピンを入れます（[自分のボード（マイコンで選ぶ）](/ja/reference/boards/)を参照）。


`topology.yaml` のそのノードから `board:` を外し、ボードの値を `dagon.toml` に書きます:

```yaml
  - id: seg
    mcu: stm32f401re
    role: node
    port: self-boot
    flash_map: f401-default
    tag: seg-f401-7seg
```

```toml
[board.seg]
flash_bytes = 524288
```

I2C の子機では、リンクにノード側のピン（`node_pins`）を書きます。足りないものは検査と `dagon gen` が知らせます。`add-a-board-by-mcu` スキルを使えば Claude と一緒に進められます。
