---
title: "制限事項"
description: "設計との違いと、DAGON がしないことです。仕様上の制限と、今後解決する予定の制限に分けています。"
---

## 仕様上の制限

- 署名の鍵をどう守るかは利用者が選びます。DAGON は決めません。ハードウェアの信頼の起点（セキュアブートのヒューズやハードウェアの鍵保管）を使うかどうかも、その選択です。

## 今後解決する予定

親機とブート:

- 親機にできるのは RP2040 だけです。リレーのコードはそのマイコンの定義の中にあります。
- 書き込めるのは親機とその先の子機までです。子機より先の孫機には書き込めません。
- スロットが 1 つの配置はありません。
- ブートローダは通信せず、アプリを書き込みません。
- ESP32-S3 は公式の ESP-IDF の 2nd stage を使います。その 2nd stage 自体が壊れた場合、DAGON では救えません。ベンダーのツールで USB から書き直す必要があります。

拡張の隔離:

- ESP32-S3 の隔離のレベルは `none` です。PMS と WCL のレジスタをプログラムするコードがありません。
- 実行時のサンドボックスはありません（Service ABI、Entry Gate、Operation Cancellation、Fault Unwind、Lockup Fuse、Reset Cleanup、SandboxArch）。定義の検査と保護マップの生成はあります。
- 保護マップを生成できる種類は `armv6m-mpu` と `armv7m-mpu` だけです。

リンク:

- ファームウェアがあるリンクの種類は `usb-cdc`、`uart`、`i2c` だけです。`ble`、`wifi-tcp`、`ethernet`、`can` にはありません。

署名の鍵:

- 署名の鍵はファイルからだけ読みます。環境変数 `DAGON_SIGNING_KEY` が持つのもファイルのパスです。ほかの手段（環境変数、OS のキーストア、1Password など）をポートで選ぶことはできません。

値と ID:

- USB の VID:PID `c0de:cafe` は試験用の ID で、コードに固定されています。割り当てを受けたものではありません。
- DAGON のマイコンの定義には、出典を確かめていない値があります。定義の中で `unverified` と示します。
- 実行モデルの上限（[12 節](/explanation/lifecycle-events/#12-実行モデル)）は、実機で測定した値ではありません。
- ブートローダの試行のセルが値を保つことは、ベンダーの文書だけによります。ボードごとには確かめていません。

設計の図にあってコードに無いコンポーネント:

- 置き換えのトレイトが無いポート: BuildRunner、Signer、DecisionPolicy、LogStorage、UpdateTransport、UpdateProtocol、Recovery、ChildLink、ImageVerifier、Digest、CryptoProvider、Diagnostics、Display、LogSink、Agent、Action
- 親機: Coordinator、Release Store、Bus Guardian。書き込みは PC が親機を通して進めます。
- 書き込みの計画: Delivery Plan の試行の窓と昇格の判断
- 復旧: Recovery Policy と Node Recovery の、段階の自動の選択と回数に上限のある再試行
- ホスト: Host Extension Chain、Action Definitions
- ブート: Boot Record Writer、Boot Action Interpreter
- Retained Codecs: ファームウェアにはありません。実行可能なモデルの中にだけあります。
