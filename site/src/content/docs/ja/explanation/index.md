---
title: DAGON のしくみ
description: DAGON の書き込み、形式、拡張の考え方です。
sidebar:
  order: 0
---

DAGON は、構成の中のすべてのノードの起動と書き込みを受け持ちます。
PC の USB につないだ親機が、新しいバージョンを I2C か UART で子機へ渡します。
ノードは新しいイメージを動いているものの隣に書き、試しに起動します。
新しいバージョンが自分で確定したときだけそれを使い続け、そうでなければ自分で前のバージョンに戻ります。

DAGON の中の状態の変化は、どれも状態機械の遷移です。遷移はどれもイベントを出します（[イベント](/ja/reference/events/)）。
拡張はそのイベントを受け取り、いくつかの判断に[ポート](/ja/reference/ports/)を通して答えます。

DAGON が読み書きする形式、プロトコル、API には、どれも番号があります。
知らない番号は、読む側が理由を示して受け付けません（[バージョンの決まり](https://github.com/c-ardinal/DAGON-Boot/blob/main/docs/versioning.md)）。
