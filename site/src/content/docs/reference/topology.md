---
title: "構成のリンク"
description: "topology.yamlのlinks: 2つの端でノードをつなぎ、向きはありません。"
---

構成（`topology.yaml`、`dagon: 2`）の`links`は、2つのノードをつなぐリンクの一覧です。リンクに向きはありません。`ends`の2つの端は同じ重みで、どちらを先に書いても確認、書き込みの届く範囲、コンパイル済みの構成は同じです。どのノードがどのノードへ届くかは、PCがつながるノードから決まります。

端（`ends`の各項目）は、ノードと、そのノードのマイコンの定義の`[[link_port]]`の名前（[マイコンとボードの定義](/reference/definitions/)）を書きます。そのポートの種類はリンクの`kind`と同じです（V-27）。アドレスのある種類（I2C）では、ほかの端がそのノードへ届くアドレスを、その端の`addr`に書きます。ノードごとのピン（`node_pins`、`pins`、`nrst`、`boot`）は、そのピンを持つノードの端に書きます。コンパイル済みの構成は、リンクごとに両方の端のノード、ポートの番号、アドレスを持ちます。`buses`は配線の確認のために残り、リンクをファームウェアやホストに示すものではありません。

```yaml
links:
  - kind: i2c
    bus: i2c-main
    ends:
      - { node: hub, port: i2c0, nrst: 18, boot: 28 }
      - { node: seg, port: i2c3, addr: 0x42, node_pins: { sda: PB4, scl: PA8 } }
    control:
      kind: full
      rom_entry: stm32-i2c-rom
```

## リンク

<!-- generated from dagon-topology's Link: python site/scripts/gen.py -->
| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `kind` | 文字列 | 必須 | 媒体: `i2c`、`uart`、`can`、`ethernet`、`usb-cdc`、`ble`、`wifi-tcp`。 |
| `ends` | 2つの端の配列 | 必須 | つなぐ2つのノード。それぞれ、そのノードで使うポートと一緒に書きます（V-04、V-27）。 |
| `bus` | 文字列 | 任意 | I2Cのリンクのバス: `buses`の`id`（V-01）。 |
| `iface` | 文字列 | 任意 | UARTのリンクのUART: `hw-uart0`か`pio-uart`（V-01）。 |
| `baud` | 整数 | 任意 | UARTのリンクのビットレート。書かないときは115200です（V-26）。 |
| `control` | テーブル | 任意 | このリンクでノードをROMのローダーに入れる方法: `kind`（`full`、`cooperative`）と`rom_entry`（`force-download-boot`、`m5pm1`、`stm32-i2c-rom`）。I2CかUARTのリンクは、既定のcooperativeのとき`rom_entry`が要ります（V-11）。 |
<!-- end of generated table -->

## 端

<!-- generated from dagon-topology's End: python site/scripts/gen.py -->
| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `node` | 文字列 | 必須 | ノードの`id`。 |
| `port` | 文字列 | 必須 | そのノードのマイコンの定義の`[[link_port]]`の名前。リンクの種類のものです（V-27）。 |
| `addr` | 整数 | 任意 | もう一方の端がこのノードへ届く7ビットのアドレス（0x08から0x77）。アドレスのある種類（I2C）だけで、ほかの種類では受け付けません（V-18）。1つのバスで2つの端が同じアドレスを使うことはできず、そのバスで届くROMのローダーのアドレスも使えません（V-18）。 |
| `node_pins` | テーブル | 任意 | このノードのI2Cのピン。マイコンのピン名で書きます（`{ sda: PB4, scl: PA8 }`）。`addr`のある端に書きます（V-01）。ボードのI2Cの組のどれかです（V-07）。 |
| `pins` | テーブル | 任意 | このノードのUARTのピン。GPIOの番号で書きます（`{ tx: 26, rx: 27 }`）。UARTのリンクは少なくとも一方の端に書き、ほかの種類のリンクにはどの端にも書きません（V-01）。 |
| `nrst` | 整数 | 任意 | もう一方のノードのリセットを動かす、このノードのGPIO。`control.kind: full`のときに使います。 |
| `boot` | 整数 | 任意 | もう一方のノードの起動ピンを動かす、このノードのGPIO。`control.kind: full`のときに使います。 |
<!-- end of generated table -->
