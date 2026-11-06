---
title: "つながりを差し替える"
description: "ノードのつながり方を変え、新しい構成を送ります。"
---

構成のホップには、それぞれ種類があります: `usb-cdc`、`uart`、`i2c`、`ble`、`wifi-tcp`、`ethernet`、`can`。親機は、上り（親機を動かす側へ）のつながりと、その先にあるものも書けます:

```yaml
  - id: hub
    ...
    uplink: { kind: uart, host: browser }   # pc-cli | browser | embedded
```

`uplink` を書かなければ、これまでどおりパソコンの dagon への `usb-cdc` です。上りのつながりはコンパイルした構成に入らないので、今ある構成とそのリリースは変わりません。DAGON Console の準備では、インスペクタでリンクごとの種類と、親機の上りのつながりを選べます。どちらかの端のファームウェアがまだ実装していない種類には ◇ まだ使えない が付きます。構成には書けます（問題「まだ使えないつながり」）が、`dagon gen` はそのホップの名前を挙げて断ります（`uplink of hub over ble: not available yet on rp2040`）。ファームウェアにあるつながりの種類（各 MCU の定義の `[links]`。上り: パソコンや親機へ、下り: 子機へ）は、[Boards and MCUs](/ja/reference/boards/) の Up links と Down links の列にあります。

デモでは、新しい無線なしで差し替えを見られます:

```sh
dagon demo --uplink uart          # または usb-cdc（既定）、wifi-tcp
```

模擬の上りのつながりはデモの構成に従います。準備で親機の上りのつながりを変える（USB、UART、Wi-Fi）と、同じ書き込みが別の媒体で動きます。媒体ごとに、フレームの大きさ、速さ、親機の再起動中のふるまい（USB CDC と Wi-Fi はつなぎ直し、UART アダプタはつながったまま）が違います。
