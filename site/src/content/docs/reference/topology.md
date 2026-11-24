---
title: "構成のリンク"
description: "topology.yamlのlinks: 2つの端でノード間を接続し、方向はありません。"
---

構成（`topology.yaml`、`dagon: 2`）の`links`は、2つのノード間を接続するリンクの一覧です。リンクに方向はありません。`ends`の2つの端は対等です。記述の順序に関係なく、確認、書込の到達範囲、コンパイル済の構成は同一です。各ノードの到達先は、PCに接続したノードを起点に決定します。

端（`ends`の各項目）にはノードと、そのノードのマイコン定義にある`[[link_port]]`の名称（[マイコンとボードの定義](/reference/definitions/)）を記述します。そのポートの種類はリンクの`kind`と同一です（V-27）。アドレスのある種類（I2C）では、対向側の端からそのノードへ到達するアドレスを、その端の`addr`に記述します。ノードごとのピン（`node_pins`、`pins`、`nrst`、`boot`）は、そのピンがあるノードの端に記述します。コンパイル済の構成は、リンクごとに両端のノード、ポートの番号、アドレスを保持します。`buses`は配線の確認用に残存し、ファームウェアやホストにリンクを提示するものではありません。

リンクの`kind`、`control.rom_entry`、コーディネータの`uplink.host`は文字列です。このDAGONにない名称は、対応済の名称の一覧と共に名称を明示して拒否します（V-01: ``this DAGON does not know the link kind `lora` ``）。そのため、対応する種類が多数のDAGON用の構成と、形式が不正な構成を区別可能です。名称を追加しても形式の番号は不変で、`dagon: 2`のままです。

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
| `ends` | 2つの端の配列 | 必須 | 接続する2つのノード。各ノードについて、そのノードで使用するポートも記述します（V-04、V-27）。 |
| `bus` | 文字列 | 任意 | I2Cのリンクのバス: `buses`の`id`（V-01）。 |
| `iface` | 文字列 | 任意 | UARTのリンクのUART: `hw-uart0`か`pio-uart`（V-01）。 |
| `baud` | 整数 | 任意 | UARTのリンクのビットレート。省略時は115200です（V-26）。 |
| `control` | テーブル | 任意 | このリンクでノードをROMのローダーに移行させる方法: `kind`（`full`、`cooperative`）と`rom_entry`（`force-download-boot`、`m5pm1`、`stm32-i2c-rom`）。I2CかUARTのリンクは、既定のcooperativeの場合に`rom_entry`が必要です（V-11）。 |
<!-- end of generated table -->

## 端

<!-- generated from dagon-topology's End: python site/scripts/gen.py -->
| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `node` | 文字列 | 必須 | ノードの`id`。 |
| `port` | 文字列 | 必須 | ノードのマイコン定義にある`[[link_port]]`の名称。リンクの種類に該当する名称です（V-27）。 |
| `addr` | 整数 | 任意 | 対向側の端がこのノードへ到達する7ビットのアドレス（0x08から0x77）。アドレスのある種類（I2C）専用で、他の種類では拒否します（V-18）。1つのバスで2つの端が同一のアドレスを使用することは不可で、そのバス経由で到達するROMのローダーのアドレスも使用不可です（V-18）。 |
| `node_pins` | テーブル | 任意 | このノードのI2Cのピン。マイコンのピン名で記述します（`{ sda: PB4, scl: PA8 }`）。`addr`のある端に記述します（V-01）。ボードのI2Cの組のいずれかです（V-07）。 |
| `pins` | テーブル | 任意 | このノードのUARTのピン。GPIOの番号で記述します（`{ tx: 26, rx: 27 }`）。UARTのリンクは最低1つの端に記述し、他の種類のリンクはどの端にも記述しません（V-01）。 |
| `nrst` | 整数 | 任意 | 対向ノードのリセットを駆動する、このノードのGPIO。`control.kind: full`の場合に使用します。 |
| `boot` | 整数 | 任意 | 対向ノードの起動ピンを駆動する、このノードのGPIO。`control.kind: full`の場合に使用します。 |
<!-- end of generated table -->
