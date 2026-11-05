---
title: DAGON を試す
description: 模擬の親機と子機で動く DAGON Console を、ブラウザか dagon コマンドで試します。
sidebar:
  order: 0
---

## ブラウザで

[DAGON Console のデモ](~/demo/)は、模擬の親機 1 つと子機 2 つを相手に、ブラウザの中で DAGON Console を動かします。
見本から構成を作り、「書込」に切り替えて「接続する」を押し、用意してある新しいバージョンをノードへ書き込めます。
デモはファイルを読み書きせず、機器につながず、鍵を持ちません。
そのための操作は、指したときに理由を表示します。

## dagon コマンドで

`dagon` を入れてあれば、同じ模擬のノードを自分の PC で動かせます。

```sh
dagon demo
```

`dagon demo` は、模擬の親機と子機、デモ専用のプロジェクトと鍵で DAGON Console を開きます。

## DAGON を使えるようにする

入れるツールと最初のプロジェクトは [README](https://github.com/c-ardinal/DAGON-Boot/blob/main/README.ja.md#必要なもの) にあります。
