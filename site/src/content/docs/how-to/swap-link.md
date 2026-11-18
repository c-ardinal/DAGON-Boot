---
title: "つながりを差し替える"
description: "ノードのつながり方を変えます。"
---

## 構成に書く

<!-- generated from dagon_format::with_link_kinds!: python site/scripts/gen.py -->
構成のリンクには、それぞれ種類があります: `usb-cdc`、`uart`、`i2c`、`ble`、`wifi-tcp`、`ethernet`、`can`。
<!-- end of generated table -->

PC接続ノードには、上りのつながり（そのノードを動かす側へのつながり）と、その先にあるものも書けます。

```yaml
  - id: hub
    ...
    uplink: { kind: uart, host: browser }   # pc-cli | browser | embedded
```

`uplink`を書かなければ、PCのdagonへの`usb-cdc`です。

上りのつながりは、コンパイルした構成に入りません。そのため、今ある構成とそのパッケージは変わりません。

## DAGON Consoleで選ぶ

「準備」のインスペクタで、リンクごとの種類と、PC接続ノードの上りのつながり（そのマイコンの定義が上りに持つ種類）を選べます。

## 使える種類

リンクの一方の末端のファームウェアが実装していない種類には「（使用不可）」を表示します。

- 構成には記述できます（問題「使えないつながり」）。
- `dagon gen`は、その接続の名前を表示して拒否します（`uplink of hub over ble: the firmware of rp2040 does not run this link kind`）。

ファームウェアにある種類は、[ボードとマイコン](/reference/boards/)の「上りリンク」と「下りリンク」の列にあります。上りはPCやPC接続ノードへ、下りは接続先のノードへのつながりです。元のデータは各マイコンの定義の`[links]`です。
