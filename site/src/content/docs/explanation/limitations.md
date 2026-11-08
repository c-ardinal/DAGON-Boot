---
title: "制限事項"
description: "設計との違いと、DAGONがしないことです。仕様上の制限と、今後解決する予定の制限に分けています。"
---

## 仕様上の制限

- 署名の鍵をどう守るかは利用者が選びます。DAGONは決めません。ハードウェアの信頼の起点（セキュアブートのヒューズやハードウェアの鍵保管）を使うかどうかも、その選択です。

## 今後解決する予定

親機とブート:

- 親機にできるのはRP2040だけです。リレーのコードはそのマイコンの定義の中にあります。
- 書き込めるのは親機とその先の子機までです。子機より先の孫機には書き込めません。
- USBハブによっては、親機が自分に新しいバージョンを書き込んで再起動したあと、PCが親機を再び認識しないことがあります。親機はPCのUSBポートに直接つないでください。
- スロットが1つの配置はありません。
- ブートローダは通信せず、アプリを書き込みません。
- ESP32-S3は公式のESP-IDFの2nd stageを使います。その2nd stage自体が壊れた場合、DAGONでは救えません。ベンダーのツールでUSBから書き直す必要があります。

拡張の隔離:

- ESP32-S3の隔離のレベルは`none`です。PMSとWCLのレジスタをプログラムするコードがありません。
- 実行時のサンドボックスはありません（Service ABI、Entry Gate、Operation Cancellation、Fault Unwind、Lockup Fuse、Reset Cleanup、SandboxArch）。定義の検査と保護マップの生成はあります。
- 保護マップを生成できる種類は`armv6m-mpu`と`armv7m-mpu`だけです。

リンク:

- ファームウェアがあるリンクの種類は`usb-cdc`、`uart`、`i2c`だけです。`ble`、`wifi-tcp`、`ethernet`、`can`にはありません。

識別とVERSION:

- マイコンの識別は手で割り当てた16ビットの`wire_id`です。イメージのヘッダ、識別の返答、VERSIONの0x06、構成のノードが持ちます。設計の、名前のハッシュから導く64ビットの識別ではありません。
- VERSIONが答えるのはid 0x01、0x06、0x0A、0x0B、0x0C、0x0Dだけです。空でないリクエストは拒みます。

署名の鍵:

- 署名の鍵はファイルからだけ読みます。環境変数`DAGON_SIGNING_KEY`が持つのもファイルのパスです。ほかの手段（環境変数、OSのキーストア、1Passwordなど）をポートで選ぶことはできません。

値とID:

- USBのVID:PID `c0de:cafe`は試験用のIDで、コードに固定されています。割り当てを受けたものではありません。
- DAGONのマイコンの定義には、`unverified`の印が付いた値があります。
- 実行モデルの上限（[12節](/explanation/lifecycle-events/#12-実行モデル)）は、12.3の決め方で決めた値ではありません。

設計の図にあってコードに無いコンポーネント:

- 置き換えのトレイトが無いポート: BuildRunner、Signer、DecisionPolicy、LogStorage、UpdateProtocol、Recovery、ChildLink、ImageVerifier、Digest、CryptoProvider、Diagnostics、Display、LogSink、Agent、Action
- 親機: Coordinator、Release Store、Bus Guardian。書き込みはPCが親機を通して進めます。
- 書き込みの計画: Delivery Planの試行の時間枠と昇格の判断
- 復旧: Recovery PolicyとNode Recoveryの、段階の自動の選択と回数に上限のある再試行
- ホスト: Host Machines、Host Extension Chain、Action Definitions
- ブート: Boot Record Writer、Boot Action Interpreter
- Retained Codecs: ブート記録とロックアップヒューズの形式は、実行可能なモデルの中にだけあります。ファームウェアにはありません。
- 構成の本体: ファームウェアの形式は`dagon-format`のもの（先頭32バイト、ノード32バイト）で、[保持と永続の形式](/explanation/retained-formats/)の形式ではありません。親機は1つの領域に保存し、2バンクの置き換えはありません。
