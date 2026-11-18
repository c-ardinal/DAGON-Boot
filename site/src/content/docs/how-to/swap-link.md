---
title: "リンクの交換"
description: "ノードの接続方法を変更します。"
---

## 構成への記述

<!-- generated from dagon_format::with_link_kinds!: python site/scripts/gen.py -->
構成のリンクには、それぞれ種類があります: `usb-cdc`、`uart`、`i2c`、`ble`、`wifi-tcp`、`ethernet`、`can`。
<!-- end of generated table -->

コーディネータには上りのリンク（そのノードを操作する側へのリンク）と、その接続先も記述可能です。

```yaml
  - id: hub
    ...
    uplink: { kind: uart, host: browser }   # pc-cli | browser | embedded
```

`uplink`の記述がない場合は、PCのdagonへの`usb-cdc`です。

上りのリンクはコンパイルした構成の対象外です。そのため既存の構成とそのパッケージは変化しません。

## DAGON Consoleでの選択

「準備」のインスペクタで、リンクごとの種類と、コーディネータの上りのリンク（そのマイコンの定義が上りに対応する種類）を選択可能です。

## 使用可能な種類

リンクの一端のファームウェアが未実装の種類には「（使用不可）」を表示します。

- 構成には記述可能です（問題「使えないつながり」）。
`dagon gen`はその接続の名称を表示して拒否します（`uplink of hub over ble: the firmware of rp2040 does not run this link kind`）。

ファームウェアが対応する種類は、[ボードとマイコン](/reference/boards/)の「上りリンク」と「下りリンク」の列にあります。上りはPCやコーディネータへのリンク、下りは接続先のノードへのリンクです。元データは各マイコンの定義の`[links]`です。
