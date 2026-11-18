---
title: "マイコンとボードの定義"
description: "dagon-mcu.tomlとdagon-board.toml: DAGONが把握するマイコンやボードの情報です。"
---

各マイコンの定義（`targets/mcu/<id>/`）には`dagon-mcu.toml`が、各ボードの定義（`targets/boards/<id>/`）には`dagon-board.toml`があります。このファイルに、DAGONが把握するマイコンやボードの情報を記述します。DAGONのコードにあるのは、種類の閉集合と、それを使用するエンジンだけです。`dagon-topology::defs`がこれらを読込して検証し、`Registry`に変換します。

- `Registry::builtin()`: このツリーの全`targets/mcu/*/dagon-mcu.toml`と全`targets/boards/*/dagon-board.toml`。フォルダから検出し、ビルド時に埋め込みます（ファイルシステムは使用しません。ブラウザ用のビルドはこちらを使用します）。定義のフォルダの追加で定義が増加します。フォルダの順序に意味はありません。
- `Registry::load(&[Source::Tree(root), Source::Project(dagon_toml)])`: DAGONのソースツリーの定義が組込の定義を置換し、プロジェクトは独自の定義を追加します。プロジェクトの定義は、既存のマイコンやボードのidを使用不可です。プロジェクトは既存の定義のうち変更対象の欄だけを上書き可能です（後述の「プロジェクトの定義」）。

数値はTOMLの整数です（`0x0800_8000`、`400_000`）。未知の欄はエラーです。種類の欄は後述の値だけを受理します。

## dagon-mcu.toml

<!-- generated from dagon-topology's McuDef: python site/scripts/gen.py -->
| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `format` | 整数 | 任意 | 定義の形式番号（`3`。省略時は1）。その番号を解釈しないdagonは、ファイルを拒否します（site/src/content/docs/explanation/versioning.md）。 |
| `id` | 文字列 | 必須 | 構成で使用するマイコンのid（`stm32f401re`）。 |
| `name` | 文字列 | 必須 | 表示名（`STM32F401RE`）。 |
| `wire_id` | 整数 | 必須 | イメージのヘッダの`mcu_id`。重複しないこと。 |
| `port` | 文字列 | 必須 | `self-boot`か`vendor-stage2`。 |
| `gpio_count` | 整数 | 任意 | ボード未指定のノードで確認するGPIO0..n-1。ピンをポートで命名する場合は0。 |
| `[[flash_map]]` | テーブルの配列 | 必須 | `name`（重複しないこと）、`base`（フラッシュのバイト0のアドレス）、`bytes`、`xip`（必須。`true`: CPUがフラッシュを`base`で読込するので、フラッシュはRAMやレジスタブロックとアドレス空間を共有します。`false`: `base`とスロットは、MMUが別の位置に割当するフラッシュのオフセットです）。最初の項目が既定です。 |
| `partitions` | 文字列 | 任意 | 定義フォルダ内にあるESP-IDFパーティションテーブルのCSV（各行にオフセットとサイズの明示が必要です）。 |
| `[layout]` | テーブル | 任意 | 任意の固定リージョン: `stage0 = { at, size }`、`bootcore_log` / `app_log = { faces = [a, b], face_size }`。`bootcore_log`が`app_log`と同一の場合は、ブートローダとAPPの両方の記録を保持する1つのログです。 |
| `[stage0]` | テーブル | 任意 | マイコンのDAGONのstage0。`[layout]`に`stage0`がある場合だけ記述します: `key`（stage0がブートローダの署名の検証に使用するキーの格納先。既定で唯一の格納先は`"flash"`で、stage0のビルド時にstage0自身のフラッシュに書込する定数です。他の格納先は拒否します）、`region_bytes`（領域の全体で、`[layout] stage0`のサイズと同一です。stage0のイメージはその1/16を未使用領域として確保し、リンク処理がその確保を検証します）と`head_bytes`（必須。領域の先頭で、ブートROMがstage0に先行して読込するバイト数。RP2040のboot2などです。不要なら0。`region_bytes`未満であること）。`[[stage0.clock]]`はstage0が動作するクロックの設定を列挙します: `{ source = "internal", out_hz }`（リセット直後から動作するマイコン自身の発振器）を1つだけと、`{ source = "crystal" \| "external", in_hz, out_hz, pll = { ... } }`を任意の数。`pll`はマイコン固有の項目を保持し（RP2040: `refdiv`、`fbdiv`、`postdiv1`、`postdiv2`で、水晶から。STM32F401: `m`、`n`、`p`、`q`、`flash_wait_states`で、水晶または外部クロックから）、その項目に該当するPLLの規則で検証します。DAGONが認識するどのPLLにも該当しない項目の`pll`は拒否します。ノードのstage0は、ボードの`[clock]`の供給元（同一の種類と`in_hz`）に一致する設定のうち`out_hz`が最大の設定で動作し、一致する設定が不在ならinternalの設定で動作します。種類と`in_hz`が同一で`out_hz`も同一の設定が2つあれば拒否します。 |
| `[bootcore]` | テーブル | 任意 | DAGONのブートローダのクロック。マイコンに`bootcore`のスロットがある場合だけ記述します: `[[bootcore.clock]]`は、`[[stage0.clock]]`のstage0用の設定と同様に、ブートローダが動作する設定を列挙し、同一のPLLの規則で検証します。ブートローダのHALが選択した設定を構成します。internalの設定は、マイコン自身の発振器からPLLを駆動可能です: `{ source = "internal", in_hz, out_hz, pll = { ... } }`で、`in_hz`はその発振器の周波数です（STM32F401のHSI）。`pll`が不在の場合は`out_hz`だけを記述します。ノードのブートローダは、stage0と同様に、ボードの`[clock]`が選択する設定で動作し、一致する設定が不在ならinternalの設定で動作します。 |
| `[app]` | テーブル | 任意 | アプリのクロック。マイコンに`bootcore`のスロットがある場合だけ記述します: `[[app.clock]]`は、アプリ用の`[[bootcore.clock]]`です。選択した設定は`dagon gen`がノードの`wiring.rs`に出力し、ボードの定義は自身の定義のモジュールに出力します。マイコン定義側のアプリ起動処理がその設定を構成します。RP2040ではUSBのPLL（`usb_refdiv`、`usb_fbdiv`、`usb_postdiv1`、`usb_postdiv2`で、48MHzを生成します）を保持する設定が、アプリにUSBのクロックを付与します。USBのPLLが不在の設定のアプリは、USBの上りのリンクが不在の場合だけビルド可能です。internalの設定には`tolerance_ppm`（発振器の`out_hz`からの許容偏差）を記述可能です。この項目が不在のinternalの設定でアプリが動作するノードは、そのクロックでUARTリンクを使用しません（V-26）。 |
| `[[slot]]` | テーブルの配列 | 必須 | `image`（`app`、`bootcore`、`agent`、`stage0`）、`binding`（`a`、`b`、`any`）、`header`（`inline`、`trailer`、`detached`）。領域は`at` + `size`か`partition`です。分離したヘッダの位置は`header_at`か`header_partition` + `header_offset`です。 |
| `[writers]` | テーブル | 必須 | マイコンのファームウェアの書込範囲: `[[slot]]`が命名するイメージの種類ごとに、その種類を書込する種類を列挙します（`app = ["app"]`、`bootcore = ["app"]`）。列挙する種類はいずれも、ここにスロットを保持する種類です。ある種類は自身のスロットで動作して、別の種類のスロットに書込します。自身を列挙した種類は、自身が動作していない自身のスロットに書込します。この記述はその種類のスロットが2つの場合だけ有効です。種類ごとのスロットは1つ（`a`）か2つ（`a`と`b`）です。スロットが1つの種類は、自身以外の種類による書込の場合だけロード可能です。 |
| `[writer]` | テーブル | 必須 | 最初の書込のツール: `kind`とそのパラメータ（下記）。 |
| `[[part]]` | テーブルの配列 | 任意 | 最初の書込のために、この定義のフォルダからビルドするDAGONの部品: `name`、`sub`（定義のフォルダ内のフォルダ）、`bin`。 |
| `[coordinator]` | テーブル | 任意 | コーディネータに指定可能なマイコン専用: `pio_state_machines`、`pio_uart_state_machines`（リンクごと）、`uart0_tx`、`uart0_rx`。構成がそのマイコンをコーディネータに指定可能なのは、`[coordinator]`が定義済の場合だけです（V-25）。 |
| `[link_uart]` | テーブル | 任意 | マイコンの定義のコードがDAGONのUARTリンクに使用するUARTの、ビットレートの算出方法。HALの計算と同一です: そのクロックを、`clocks_per_bit`と`frac_bits`ビットの小数部付きの分周比との積で除算します。端数処理は切捨です。クロックは`clock_hz`（クロック設定に依存しない周波数）か`clock = "app"`（ノードの`[[app.clock]]`設定のコアクロック。V-26はノードごとにこの値を使用します。`tolerance_ppm`が不在のinternalの設定で動作するノードは、UARTリンクを使用しません）のどちらか1つです。`bauds`は、そのコードがリンクに使用するビットレートの一覧です。構成の確認（V-26）は、どちらかの端の一覧に不在か、どちらかの端で2 %超の誤差を生じるリンクの`baud`を拒否します。DAGON Consoleは両端の一覧にある速度だけを選択肢に表示します。`[links]`に`uart`があるマイコンでは必須です。この項目が不在の場合、そのマイコンのファームウェアが使用する全部のUARTリンクを拒否します。動作が判明している速度が不在のためです。 |
| `[link_i2c]` | テーブル | 任意 | マイコンの定義のファームウェアが、コントローラとしてもターゲットとしても、DAGONのI2Cリンクに使用するバスの速度の一覧: `hzs`。構成の確認（V-20）は、バス上のいずれかのノードの一覧に不在のバスの`speed_hz`を拒否します。DAGON Consoleは、バス上の全ノードの一覧にある速度だけを選択肢に表示します。`[links]`に`i2c`があるマイコンでは必須です。この項目が不在の場合、そのマイコンのファームウェアが接続する全部のI2Cバスを拒否します。動作が判明している速度が不在のためです。 |
| `[rom_i2c]` | テーブル | 任意 | ROMのI2Cブートローダ（`stm32-i2c-rom`）: `name`（確認のメッセージでの名称）、`addr`、`max_hz`、`erase_sectors`、待受対象の`pairs = [{ controller, sda, scl }]`。 |
| `[links]` | テーブル | 任意 | マイコンの定義のファームウェアが実装するリンクの種類（その`LINK_KINDS`）。 |
| `[boot_receive]` | テーブル | 任意 | アプリを書込するブート段階（`[writers] app`が`app`以外の種類を指定する場合）が、アプリの無応答時に受信する方法（省略時は、どのリンクの種類も待受時間に参加せず、ブート段階は書込モードか復旧モードでのみ受信します）: `listen_window_ms`（0から200。省略時は50。0は待受時間を開始しません）は、起動からブートまでにSTAYを待機する時間です。`kinds = { <link kind> = true \| false, ... }`は、リンクの種類ごとの待受時間への参加の有無です（記述しない種類は不参加）。対向側が先行して列挙する種類（`usb-cdc`）は参加不可です。ブート段階は`[links]`の種類の全リンクで受信し、リンク間の優先順位はありません。参加する種類では起動直後から、他の種類では書込モードか復旧モードで受信します。プロジェクトは`[mcu_override.<id>.boot_receive]`で待受時間を変更します。 |
| `[[link_port]]` | テーブルの配列 | 任意 | 構成のリンクの端が名称で指定する、マイコンのリンクのインタフェース: `index`（0から255）、`name`（`a-z`、`0-9`、`-`）、`kind`（リンクの種類: `i2c`、`uart`、`can`、`ethernet`、`usb-cdc`、`ble`、`wifi-tcp`）。`index`と`name`はそれぞれ重複しません。リンクの端の`port`はリンクの種類に該当するこれらの名称のいずれかです（構成の確認V-27）。コンパイル済の構成はその`index`を保持します。 |
| `[[relay_link]]` | テーブルの配列 | 任意 | このマイコンのノードが中継に使用するリンク。`[[link_port]]`ごとに最大1つです: `id`（RELAYのリンクの番号。1から254で、重複しません）、`port`（`[[link_port]]`の名称）、`addressing`（`"seven-bit"`: 構成が到達側の端に記述するアドレス。アドレスのある種類で使用します。`{ fixed = N }`（Nは0から255）: アドレスのない種類の、唯一の対向ノード）、`max_payload`（1から、ポート種別のプロトコル上の最大値まで）。マイコンごとのファームウェアのビルドがこの情報を`RELAY_LINKS`として生成し、PCも同一の項目を読込します。リレーはリンクごとに、`max_payload`の1フレーム分（`RELAY_LINK_BUFFERS`）のバッファだけを保持します。そのバッファにリンク種別で最大のフレームを収容不可の場合は、ビルドが停止します。バッファを超過するデータ（RELAYの内部のフレーム、I2C_RAWで書込するデータと読出のサイズ）は`E_ARG`で拒否し、切除しません。I2C_RAWの上限は、復旧に使用するリンクのバッファです。 |
| `[relay]` | テーブル | 任意 | 中継するアプリのファームウェアの予算: `ram_bytes`（静的なRAM、`.bss`と`.data`の合計のバイト数。1から、マイコンのRAMのサイズまで）。`[[relay_link]]`のあるマイコンだけが記述します。CIはマイコンごとのファームウェアの`examples/relay.rs`をビルドして計測し、この値を超過すると失敗にします（`scripts/relay_ram.py`）。 |
| `[codec]` | テーブル | 任意 | 通信コーデックの予算です。`bootcore`（マイコンの全ブートローダ）と`relay`（中継ファームウェア）に、それぞれ`{ flash_bytes, stack_bytes, ram_bytes }`を記述します。各値はコーデック（`dagon_proto`）の関数と名称付きの読出専用データのフラッシュ使用量、全関数起点の最大スタック使用量、RAM上の静的データ量で、単位はバイトです。`bootcore`は対象`bootcore`の`[[slot_layout]]`があるマイコン限定、`relay`は`[[relay_link]]`があるマイコン限定で、該当イメージがあるマイコンは記述必須です。CIは各イメージをrustcのスタックフレーム記録付きでビルドして計測し、値の超過時と、呼出グラフにハンドラ行の分岐以外の間接呼出、追跡不能な呼出、再帰が存在する時に失敗します（`scripts/codec_budget.py`）。 |
| `[connection]` | テーブル | 必須 | 書込時に利用者が接続する方法: `label`、`hint`。 |
| `[[doctor]]` | テーブルの配列 | 任意 | ツールの確認: `what`、`probe`（コマンド）、`expect`（`"runs"`、`{ line = "..." }`、`{ prefix = "..." }`）、`fix`（コマンド）、`hint`、`slow`（`true`: `dagon doctor --fix`が、インストールに時間を要することを開始前に通知します）。 |
| `[ram]` | テーブル | 任意 | `{ at, size }`: マイコンのRAMの全体。イメージは、ブート記録、ヒューズのセル、拡張のリージョンより下位のRAMを取得します。 |
| `[isolation]` | テーブル | 任意 | DAGONが拡張を自身から隔離する方法（下記）。省略時は`kind = "none"`、`level = "none"`です。 |
| `[[peripheral]]` | テーブルの配列 | 任意 | レジスタブロックごと: `id`、`base`、`size`、`window_eligible`（拡張がウィンドウとして取得可能）、`bus_master`（メモリへの書込が可能）、`privileged_why`（`dma`、`flash`、`watchdog`、`clock`、`reset`、`irq-routing`、`pinmux`、`power`、`rom`、`core-control`、`protection`、`shared`の1つか一覧）、`dma = { streams, channels }`（DMAコントローラ、つまりバスマスタ: そのストリーム。各ストリームは`channels`個の要求から1つを選択します）、`dma_requests = [{ signal, controller, stream, channel }]`（マイコンのDMA要求の割当表のうち、このブロックの行: `signal`は`a-z 0-9`で`tx`、`rx`、`ch1`、`up`など。1つのセルは1つのブロック専用です）。ボードの定義とマイコンのテンプレートは、この情報からDMAのストリームを取得します（未使用のストリームのうち、その要求の最初の行）。 |
| `[[core]]` | テーブルの配列 | 任意 | プロセッサのコアごと、`id`。DAGONは最初のコアで動作します。 |
| `[watchdog]` | テーブル | 任意 | `kind`（`iwdg`、`rp-watchdog`、`rtc-wdt`）、`trial_ms`（試行のブートで有効化）、`confirmed_ms`（それ以外で有効化）、`trial_attempts`（ブート段階がロールバックまでに待機中のイメージに許可するブートの回数: ブートローダの上限か、ベンダーの第2段階の上限）。アプリが再起動を遅延可能な時間は、短期側の周期の半分までです。マイコンの定義のビルドが、`trial_attempts`をブートローダとエージェントが使用する定数に変換します。`flag`（必須）: ロックアップヒューズに必要なウォッチドッグによるリセットのフラグ（ウォッチドッグによるリセットで設定され、電源投入以外の以降のリセットでも保持され、起動処理の確認で消去される）で、`none`、`register`（リセットのフラグのレジスタ。名称と出典をコメントで明示します）、`package`（マイコンの定義のコードが保持）のいずれかの事実です: `"register"`、`{ unverified = "package" }`、`"unmeasured"`。出典のあるフラグが不在のマイコンには、ロックアップヒューズはありません。`range_ms`（必須）: `{ min, max }`。そのマイコンのウォッチドッグが動作可能な周期（ms）で、出典をコメントで明示します。`trial_ms`か`confirmed_ms`がその範囲外の定義は、定義の読込時と、ファームウェアの配置の生成時に拒否します。 |
| `[execution]` | テーブル | 任意 | 実行モデルのハードウェアの事実（site/src/content/docs/en/explanation/lifecycle-events.mdの「Execution model」）: `masked_max_us`（DAGON自身のコードが割込を禁止する最長時間、1..=1000）、`flash_op`（消去や書込で停止するもの: `fetch-stall`、`xip-off-masked`、`cache-off-masked`）、`write_unit`（必須。フラッシュへの1回の書込の対象バイト数。2のべき乗で、全消去単位の約数であること）、`erased_value`（必須。消去済のフラッシュの読出値）、`erase_regions = [{ start, len, unit, unit_max_ms }]`（DAGONが消去するフラッシュ: 領域ごとの消去単位と、その単位の消去に要するデータシート上の最大時間。単位の整数倍で、重複しないこと。`unit_max_ms`の2倍が`watchdog.trial_ms`と`watchdog.confirmed_ms`以下であること）。事実とは、出典と照合した数値（コメントで出典を明示します）、`{ unverified = N }`、または`"unmeasured"`（値なし。`unit_max_ms`には使用不可）です。 |
| `[[slot_layout]]` | テーブルの配列 | 任意 | ノードが受信するXFERの対象とスロットの全部: `target`（`app`、`bootcore`、`agent`。廃止した`topology`は拒否します）、`slot`（`a`、`b`、`factory`）、`start`、`capacity`（ヘッダを除外した本体の最大サイズ）、`header`（`start`の位置の`"inline"`か、`{ detached = <address> }`）、`partition`（任意: 本体の領域に該当するパーティション。パーティションの表と照合します）。対象とスロットごとに1つで、`[execution]`と同時に記述します。`[[slot]]`のイメージはそれぞれ固有の項目を保持し、最大の本体の消去範囲は`erase_regions`の範囲内です。ノードもホストも、全部の消去をこの情報から計画します。 |
| `[conformance]` | テーブル | 任意 | `scratch = { at, size }`: 適合試験のビルドが消去と書込を実行するフラッシュ。どのスロット、ヘッダ、ログの面、stage0とも分離します。 |
| `[boot]` | テーブル | 任意 | `[boot.record]`、DAGONのブート段階を保持するマイコン用: ブート記録とロックアップヒューズの格納先。`kind`（`ram`の中の`noinit-ram`、`[[peripheral]]`の中の`retained-registers`）、`at`、`size`（最大4096）、`survives`（`reset-random`、`reset-cleared`、`power`: 電断後に残存するもの）、`entropy = { source, min_bits }`（必須。新規の記録のシーケンスが混合する起動時のワード: `source`は`rng`、`ring-osc`、`osc-capture`、`adc-noise`のいずれか。`min_bits`はワードあたりの最小エントロピーで、事実です。値がある場合は16以上）、`rings = [{ writer, at, size }]`（ブート段階ごとに1つ、ブートの順番に、記録の内部）、`fuse = { cells = [{ image, at, size }] }`（`bootcore`、`app`。記録の外部）、`reserved`（記録が一切ストアしないアドレス）。内部のバイトのレイアウトは`boot-record`の規約に準拠します。 |
| `boot_capabilities` | 文字列の配列 | 任意 | ボードのブートのアクションプログラムが、ピン以外に使用可能なもの: `["i2c"]`で`bus_write`を使用可能です。 |
<!-- end of generated table -->

スロットの配置: `inline`は512バイトのヘッダをスロットの先頭に配置し、その後にペイロードを配置します。`trailer`はペイロードをスロットの先頭に配置し、ヘッダを最後の512バイトに配置します。`detached`はヘッダを別の位置に配置し、ペイロードがスロット全体を占有します。`any`のスロットはスロットAとして応答します。

スロットの数: イメージの種類ごとに、`[[slot]]`が列挙する1つ（`a`）か2つ（`a`と`b`）です。`a`がない`b`は読込しません。2つの場合、新規のイメージはその種類が動作していない側のスロットに格納し、以前のバージョンはもう一方に残存します。1つのスロットを読込する条件は、`[writers]`がその種類以外に書込する種類を列挙する場合だけです。イメージは自身が動作中のスロットを上書き不可能なためです。それ以外の定義は読込せず、その種類と、このマイコンには他にその種類を書込可能なイメージがない旨を通知します。書込する種類にもそれぞれこのマイコンにスロットがあります。スロットが1つの場合に不可能なこと（以前のバージョン、書込中の電断、復旧用のイメージ）は[制限事項](/explanation/limitations/)にあります。

書込ツールの種類:

- `uf2-drive`: `family`、`sector`、`info_file`、`info_header`、`board_id`（ブートROMのドライブは、情報ファイルが`info_header`で開始し、`Board-ID: <board_id>`の行があるものです）。
- `probe-rs`: `chip`。
- `espflash`: `chip`、`flash_size`、`table_at`、`bootloader`、`checksums`、`usb_hint = { vid, pid }`。ヒントは書込ツールが報告する候補へのラベルの付与だけに使用し、候補の絞込はしません。一致する候補を先頭に表示し、事前に選択します。

### 消去と試行の待機時間

`[execution] erase_regions`はDAGONが消去するフラッシュを提示します。領域（`start`、`len`）ごとに`unit`バイト単位で全体を消去します。フラッシュの操作1回で1単位を消去し、1単位の所要時間は最大で`unit_max_ms`です。アドレスによって単位が異なるマイコン（STM32F401の16、64、128KiBのセクタ）は、単位のサイズごとに1つの領域を列挙します。フラッシュがボードの部品であるマイコン（RP2040とESP32-S3）では、マイコンの領域は、全候補の部品に共通の消去方式を使用する既定値です。単位は4KiBのセクタで、1セクタの所要時間は、それらの部品のセクタ消去の最大時間のうち最長のものです。特定の部品を搭載するボードは、その部品を`[flash]`に記述します（`part`、コメントでデータシートの出典を明示、同一形式の`erase_regions`）。その領域はそのボードのノードではマイコンの領域を全体置換し、マイコンのウォッチドッグとスロットの配置と照合して検証します。`[[slot_layout]]`はノードが受信する対象とスロットの全部を、本体の開始位置、最大の本体、ヘッダの格納位置（先頭の`"inline"`か、ESP32-S3がdagonパーティションにヘッダを格納する場合のような`{ detached = <address> }`）と共に列挙します。

ノードの実際の領域（ボードに`[flash]`があればその領域、なければマイコンの領域）とマイコンのスロットの配置から、1つの計画用の関数（`dagon_port::timeout`）が転送時の消去を決定します。対象は転送が対象とするバイト（スロットの先頭からのinlineのヘッダと本体、またはdetachedのヘッダと本体を個別に）と、そのバイトを内包する領域の各単位です。ノードのエージェントは、その計画だけをスロットのストアに指定します。ストアは領域をボードから取得します（同一の定義からボードの定義のcrateかノードのwiring.rsに生成する`ERASE_REGIONS`で、全`BoardDef`が必ず返却します）。ホストと適合試験は、同一の定義から同一の転送を計画し、ノードが消去するリクエストの応答を、計画した単位の`unit_max_ms`の合計に1000msを加算した時間だけ待機します。ノードの試行時の待機時間は`[watchdog] trial_ms`と`trial_attempts`に由来します。ホストは1回の試行中に新規バージョンのHELLOを確認してCONFIRMを送信します。新規バージョンの待機時間は`trial_attempts`回の試行の全体を網羅します（マイコンごとの値は、作成時にその待機時間をホストの待機時間と照合して検証します）。

マイコンの定義がないノード、または`[watchdog]`がない定義のノードには、ホストと適合試験は既定値を使用します。消去の待機時間は30000ms、試行は8000msで3回です。`[execution]`がない定義のノードでは、消去の待機時間が既定の30000msです。

### `[isolation]`

<!-- generated from dagon-topology's Isolation: python site/scripts/gen.py -->
| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `level` | 文字列 | 任意 | DAGONが強制するもの: `none`、`mpu`、`pms`、`tz`。種類は`none`か1つのレベルを付与します（MPUとPMPの種類は`mpu`、`esp-pms`は`pms`、`armv8m-tz`は`tz`）。 |
| `kind` | 文字列 | 任意 | 保護のハードウェア: `armv6m-mpu`、`armv7m-mpu`、`armv8m-mpu`、`armv8m-tz`、`riscv-pmp`、`riscv-epmp`、`esp-pms`、`none`。 |
| `boot_extensions` | 真偽値 | 任意 | マイコンが、拡張を実行するDAGONのブート段階を保持します（`port = "self-boot"`の場合のみ）。 |
| `ext_ram_max` | 整数 | 任意 | 1つのイメージのサンドボックスの拡張が使用するRAMの上限。リージョンのサイズは切上で計算します（`kind`が`none`以外の場合は必須）。 |
| `root_protection` | 文字列の配列 | 任意 | `dagon provision`が設定可能なルートイメージの保護: `rdp1`（復元可能）、`rdp2`（永久）、`wrp`（復元可能）、`esp-secure-boot`、`esp-flash-encryption`（永久）。 |
| `cache` | 文字列 | 任意 | `none`、`i-cache`、`id-cache`（命令とデータ）。 |
| `[[isolation.tcm]]` | テーブルの配列 | 任意 | 密結合メモリ、`[{ at, size }]`。 |
| `[isolation.capabilities]` | テーブル | 任意 | その種類の能力の記録。ブート時にハードウェアから再読込します（不一致の場合はレベルが`none`になります）。`armv6m-mpu`: `mpu_regions`は0か8、`min_region`は256以上、`subregions_from`。`armv7m-mpu`: `mpu_regions`は8か16、`min_region`は32以上、`subregions_from`。`armv8m-mpu`: `mpu_regions`は0、4、8、12、16。`armv8m-tz`: `sau_regions`（0、4、8）、`s_mpu_regions`、`ns_mpu_regions`、`idau = [{ base, size, attribution }]`。`riscv-pmp`、`riscv-epmp`: `pmp_entries`（0、16、64）、`grain`、`na4`、`tor`。`esp-pms`: `worlds`、`gdma_channels`、`memory_regions`、`peripheral_slots`。全種類共通: `critical_section`（armv6mは`primask`、armv7mとarmv8mは`basepri`、RISC-Vは`mstatus-mie`、esp-pmsは`xtensa-level`）、`unwind`（種類ごとに`cortex-m-exception`、`armv8m-tz`、`riscv-trap`、`xtensa-world`）、`timer`（`systick`、`riscv-mtime`、`timer-group`）、`reset_cleanup`（`peripheral-reset`、`reinit`）、`background_privileged`、`fault_address`。任意で`np_max_us`と`dma_stop_max_us`。これらはDAGONの上限（50と20）の引下げだけが可能です。 |
<!-- end of generated table -->

`kind`が`none`以外の定義は、全コアと、`flash`、`watchdog`、`clock`、`reset`、`irq-routing`の各区分のブロックを最低1つ列挙します（コアが複数の場合は`core-control`も、`esp-pms`と`riscv-epmp`では`protection`も）。バスマスタのウィンドウ化は常に不可能です。ウィンドウ化が不可能なブロックは、全部その理由を明示します。区分が欠落した定義は、その区分の名称を表示して拒否します。

## dagon-board.toml

<!-- generated from dagon-topology's BoardDef: python site/scripts/gen.py -->
| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `format` | 整数 | 任意 | 定義の形式番号（`3`。省略時は1）。その番号を解釈しないdagonは、ファイルを拒否します（site/src/content/docs/explanation/versioning.md）。 |
| `id` | 文字列 | 必須 | `nodes[].board`で使用するボードのid。 |
| `product` | 文字列 | 必須 | 製品名。 |
| `mcu` | 文字列 | 必須 | マイコンのid（定義済であること）。 |
| `flash_bytes` | 整数 | 必須 | フラッシュのサイズ（マイコンのフラッシュマップのいずれかを収容可能なこと）。 |
| `i2c_target` | 真偽値 | 任意 | ボードがI2Cのターゲットになれるか。 |
| `drawing` | 文字列 | 必須 | UIの図。 |
| `default` | 真偽値 | 任意 | `true`: マイコンだけを指定したノードの確認に使用するボード（既定のボードが設定済の組込のマイコンについて、プロジェクトはその既定を置換不可です。マークを付与したボードが2つあると読込しません）。マイコンごとに、この指定が可能なボードは最大1つです。マイコンの唯一のボードは、この指定が不在でも既定です。ボードが複数でマークが不在のマイコンには既定がありません（そのノードはボードの事実なしで確認します）。定義の順番で決定することはありません。 |
| `[[header]]` | テーブルの配列 | 任意 | `[{ pin, label }]`。ボード上で確認実績のない`label`は省略します。構成がこのボードで指定可能なピンの全部です（コーディネータ側の`pins`・`control`・バスの`sda`/`scl`。ノード側の`node_pins`は`i2c`から選択します）。構成の確認（V-07）はそれ以外を拒否し、DAGON Consoleはこれらのピンだけを選択肢に表示します。 |
| `[[i2c]]` | テーブルの配列 | 任意 | ヘッダから使用可能な`[{ controller, sda, scl }]`の組。 |
| `[default_link]` | テーブル | 任意 | 構成で別途指定しない場合の、ノードのボードの配線: `kind`（`i2c`、`uart`、`can`、`ethernet`）、`iface`（`hw-uart0`、`pio-uart`）、`addr`、`baud`、`control`（`full`、`cooperative`）、`rom_entry`（`force-download-boot`、`m5pm1`、`stm32-i2c-rom`）。コーディネータでは省略します。 |
| `[coordinator]` | テーブル | 任意 | このボードがコーディネータの場合に、ページが新規リンクを配線するピン: `i2c = { controller, sda, scl }`（新規I2Cバス。`controller`はボードの`i2c`のいずれか）と`pio_uart = { tx, rx }`（新規PIO UARTリンク）。番号がマイコンの`gpio_count`未満のGPIOです。マイコンに`[coordinator]`があるボード専用です。 |
| `[[part]]` | テーブルの配列 | 任意 | ボードの定義自体からビルドする部品（`name`、`sub`、`bin`）。 |
| `[boot]` | テーブル | 任意 | ブート段階のボードの値: `led`（`[[pin]]`）、`led_active`（`high`、`low`）、`halt = { pattern, period_ms }`（`blink`、`code`、`off`。DAGONのstage0があるマイコンで`led`がある場合は、stage0が周期の10分の1単位でコードを点滅させるため、`blink`の周期は10 ms以上です）、`power_hold = { pin, level }`。ブートのアクションプログラムは`[[boot.action]]`です。ブートのアクションプログラム（`boot-program`形式1）: `on`（イベントのカタログがアクション対応と明示するブートイベント）と`ops`: `{ op = "led", state }`（`on`、`off`、`toggle`）、`{ op = "gpio", pin, level }`（`boot = true`の`[[pin]]`。電源保持のピンは保持のレベルのみ）、`{ op = "delay", ms }`（1から20）、`{ op = "bus_write", device, reg, bytes }`（`writable`の一覧にある`[[device]]`のレジスタ、1から8バイト。`boot_capabilities = ["i2c"]`のマイコンのみ）。イベントごとに最大32の操作と50 msで、バスの書込は5 msとして計算します。 |
| `[[pin]]` | テーブルの配列 | 任意 | ボードのピン: `pin`、`name`（ボードの定義のコードが参照する名称、`a-z 0-9 _`）、`boot`（ブートプログラムが駆動可能）、`levels`（許可するレベル。省略時は全部）。 |
| `[[bus]]` | テーブルの配列 | 任意 | ボードのバス: `id`（`a-z 0-9 - _`）、`kind`（`i2c`、`spi`、`uart`、`can`）、`controller`（マイコンの`[[peripheral]]`）、信号ごとのピン（i2cは`sda`、`scl`。uartは`tx`と`rx`の両方かどちらか。canは`tx`、`rx`。spiは`sck`と、`mosi`と`miso`の両方かどちらか）、`hz`（i2c、spi）または`baud`（uart、can。`dagon = true`のUARTバスでは、dagonが構成の速度を指定しないビルドのリンクの速度。後述）、`dagon`（`true`: DAGON自身のリンクがこのバスで動作するので、バスもそのコントローラも拡張の所有にはなりません）、`dma`（`true`: ボードの定義のコードはバスをDMA経由でオープンするか受渡します。i2c、spi、uart。txとrxのストリームは、マイコンの`dma_requests`からバスの順番に取得し、ボードの定義のビルドが`<Bus>TxDma`/`<Bus>RxDma`、`take_dma!`、`bind_<bus>_dma!`として出力します）。 |
| `[[peripheral]]` | テーブルの配列 | 任意 | 拡張が宣言可能なマイコンのブロックの、ボードでの名称: `id`、`mcu`（マイコンの`[[peripheral]]`のid。`dagon = true`のバスのコントローラは指定不可）。 |
| `[[device]]` | テーブルの配列 | 任意 | ボードのバス上のデバイス: `id`（`a-z 0-9 - _`）、`bus`、`addr`、`max_transfer`（必須: 1回の転送で受信可能なデータの最大バイト数。データシートの値を使用します。バスのポートとブートプログラムはこの値を遵守します）、`regs = [{ name, reg }]`（名称付きのレジスタ、`a-z 0-9 _`）、`writable`（DAGON外部からの書込を許可するレジスタ: `regs`の番号か名称）。 |
| `[flash]` | テーブル | 任意 | フラッシュがボードの部品であるマイコンで、ボードが搭載するフラッシュの部品: `part`（コメントでデータシートの出典を明示します）と、マイコンの`[execution]` `erase_regions`と同一の形式の`erase_regions`（`start`、`len`、`unit`、`unit_max_ms`）。このボードのノードでは、マイコンの領域を全体置換します。マイコンのウォッチドッグとスロットの配置と照合して確認します。省略時はマイコンの領域です。ノードもホストも同一の領域から全部の消去を計画します。 |
| `[clock]` | テーブル | 任意 | このボードでマイコンのクロック入力を駆動するもの: `source`は`"internal"`、`{ crystal_hz = N }`、`{ external_hz = N }`のいずれかです（Nの出典の回路図かデータシートをコメントに記述します）。省略時はinternalです。stage0、DAGONのブートローダ、アプリは、それぞれマイコンのこの供給元に一致する設定（`[[stage0.clock]]`、`[[bootcore.clock]]`、`[[app.clock]]`）で動作し、一致する設定が不在ならinternalの設定で動作します。それらの段階のないマイコンでは使用しません。 |
<!-- end of generated table -->

ボードの定義はDAGONのUARTリンクを、構成のリンクの`baud`で動作させます。`dagon build`、`deploy`、`flash`は、ノードの全ビルド（アプリと復旧用のエージェント）に、そのノードのUARTリンクの速度を`DAGON_LINK_BAUD`として指定します。そのため両端が同一の値でビルドされます。この指定がないビルド（単純な`cargo build`）は、`[default_link] baud`か、ボードにある`dagon = true`のUARTバスの`baud`を使用します。

ノードのI2Cターゲットのアドレスは、構成にあるそのノードの端の`addr`です。`dagon gen`はその値を`LINK_ADDR`として、アプリの`wiring.rs`とブートローダの`boot_wiring.rs`に出力します。そのため両方ともコーディネータが指定するアドレスで応答します。構成にアドレスの指定がない場合は、両方とも`[default_link] addr`を使用します。

`[[pin]]`とバス全体で、同一のピンの重複はありません。コードの生成元の名称（ピンの名称、バスのidとその信号、デバイスのid。`-`は`_`として解釈します）も、それぞれ重複しません。バスを宣言するボードでは、`[coordinator]`のピンは、`dagon = true`のI2CとUARTのバスのピンです。

ボードの定義のビルドスクリプトは、ボードの定義を読込し（`dagon_topology::defs::codegen::package_build`）、結果を取込します。`DAGON_BOOT_CRATE`がある場合の読込対象はそのフォルダの`board.toml`（`dagon gen`が出力する、プロジェクトの上書きを適用したボードの定義。後述）で、ない場合はソースツリーのボードの定義です。出力は以下のとおりです。ボードのid、フラッシュのサイズ、既定のリンク、ブートのLEDと停止のパターン、バスの速度とデバイスは定数として出力します（`def.rs`）。名称付きのピン、バス、バスのピンは、`take!(p.<name>)`マクロを付与したHALの型として出力します（`pins.rs`）。各デバイスはバスのポートの能力を表す`dagon_events::Device`として出力します（`devices.rs`。作成方法はこれだけです）。検証で読込した定義のいずれかが変化すると、ビルドを再度実行します。マイコンの定義のブートイメージは、ボードの`[boot]`を同様に読込します（`boot_build`）。読込対象はビルド対象のノードのボードです。`DAGON_BOOT_CRATE`がノードのブート用crate（プロジェクトの`boot/<id>/`。`dagon gen`が出力します）を指し、その`Cargo.toml`がノード、マイコン、ボード（このツリーのボードか、プロジェクトの`[board.<id>]`）、信頼されたブート拡張を指定します。この指定がないビルドは失敗します。ボードがないノードには、ブートのLEDも停止のパターンもありません。コード自体にはピンもアドレスもパターンもありません。

マイコンの定義のビルドスクリプトは、ノードのビルドに指定されたマイコンの定義から配置を生成します（`dagon_mcu_build::definition`）。対象はイメージの種類ごとの全スロットのアドレスとバイト長、stage0の領域、ログ、消去の領域とスロットの配置、ウォッチドッグと試行の値です。その定義は`boot/<id>/mcu.toml`で、`dagon gen`が全ノードについて出力します。内容は組込の定義にプロジェクトの上書きを適用したものです。ボードがあるノードには、同様に上書きを適用したボードの定義`board.toml`も出力します。さらに`dagon gen`は出力したファイルごとに1行を`defs.sha256`に出力します。行の内容はファイルのSHA-256（小文字の16進数64桁）、空白2つ、ファイル名で、UTF-8、改行はLFです。ノードの定義を読込するビルド（マイコンとボードのビルドスクリプト、`dagon build`、`deploy`、`flash`）は、行と不一致のファイルや、`defs.sha256`がないフォルダを拒否します。ノードの定義の変更時は、`dagon.toml`の上書きを修正して`dagon gen`を再度実行します。アプリとブート用crateの`.cargo/config.toml`は、両方とも`DAGON_BOOT_CRATE`にそのフォルダを設定します。`dagon build`、`deploy`、`flash`も、ノード用にビルドする全イメージに、そのフォルダを指定します（ボードの部品、例えば復旧用のエージェントも対象です）。そのため1つのノードの全イメージは同一のファイルからビルドされます。スロットの移動、スロットごとのバイト長の変更、ウォッチドッグの周期の変更をするプロジェクトは、コードを修正せずに適合するファームウェアを取得します。マイコンか定義の形式が異なる`mcu.toml`は拒否します。`DAGON_BOOT_CRATE`がないビルド（パッケージの単純な`cargo build`）は、パッケージ自身の`dagon-mcu.toml`を読込します。

## このPCの外部に送信するもの

定義ファイルは秘密情報の記述場所ではありません。定義内の名称（id、製品名、ピンのラベル、パーティション名）はDAGON Consoleに表示し、Claudeに依頼した場合はClaudeにも送信します。コマンド、ヒント、コメント、ファイルのパスは送信しません。プロジェクト自身の名称は、40文字以内の`A-Z a-z 0-9`・空白・`.`・`_`・`-`だけの場合は無変更で送信し、それ以外は`project-text`と表示します。

## プロジェクトの定義

プロジェクトの`dagon.toml`は、先頭に`format = 1`を記述します。これは`dagon.toml`の形式番号です（[規約と部品](/reference/contracts/)の`project`）。`format`がないファイルや別の番号のファイルは、このdagonが読込可能な番号を表示して拒否します。`dagon.toml`の最上位のキーは以下のものだけで、他のキーは拒否します。

<!-- generated from dagon-topology's project KEYS: python site/scripts/gen.py -->
`format`、`topology`、`release`、`provision_dir`、`pubkey`、`signing_key`、`provision`、`build`、`dagon`、`extensions`、`board`、`mcu_files`、`mcu_override`、`board_override`
<!-- end of generated table -->

`dagon.toml`ではボードを`[board.<id>]`のテーブルとしてその場で定義し（`id`を除外したボードのスキーマ）、マイコンのファイルを`mcu_files = ["defs/my-mcu/dagon-mcu.toml"]`で登録可能です。定義が参照するファイルは、その定義のフォルダ内で解決します。

`[board.<id>]`の`<id>`はボードのidで、ノードの`board:`がこのidを指定します。`dagon.toml`の`[board.<id>]`は常にボードの定義です。そのボードにはボードのcrateがないため、そのボードを指定するノードは、`dagon gen`がマイコンのcrateから作成し、ボードの`flash_bytes`を`wiring.rs`に出力します。マイコンだけを指定するノードにはボードは不要です。ボードもフラッシュのサイズも要求せず、DAGON Consoleは、マイコンの定義にある既定のフラッシュマップが配置するフラッシュのサイズを、読取専用で表示します。

### 定義の上書き

既存の定義（組込の定義か、DAGONのソースツリーの定義）の一部を変更する場合は、定義を複製せずに、変更する欄だけを`dagon.toml`に記述します。`[mcu_override.<id>]`はマイコンの定義を、`[board_override.<id>]`はボードの定義を上書きします。内容はその定義と同一のスキーマで、変更する欄だけを記述します。上書きはプロジェクト全体に、idごとに適用します。そのマイコンやボードを使用する全ノードが、上書きを適用した定義を使用します。記述しない欄は上書き前の定義のままです。そのためDAGONの新規リリースで上書き前の定義が変化すると、その欄も変化します。一部のノードだけで異なる定義を使用する場合は、新規idで定義を登録します（`mcu_files`、`[board.<id>]`）。プロジェクト自身の定義は上書きせず、その定義を修正します。存在しないidの上書きは拒否します。

例: RP2040のブートローダのスロットを1つにして、空いた128KiBをアプリのスロットAに追加します。`dagon.toml`に以下を追加します。

```toml
[[mcu_override.rp2040.slot]]
image = "bootcore"
binding = "b"
dagon-remove = true

[[mcu_override.rp2040.slot]]
image = "app"
binding = "a"
at = 0x1002_8000
size = 0x12_0000

[[mcu_override.rp2040.slot_layout]]
target = "bootcore"
slot = "b"
dagon-remove = true

[[mcu_override.rp2040.slot_layout]]
target = "app"
slot = "a"
start = 0x1002_8000
capacity = 0x11_FE00
```

スロットはキー（`image`と`binding`）で、受信の配置はキー（`target`と`slot`）で、既存のエントリを特定します。アプリのスロットAで変化するのは記述した欄だけで、`header`など他の欄は上書き前の定義のままです。ブートローダのスロットが1つでも読込可能な理由は、`[writers]`でアプリがブートローダを書込するためです。スロットが1つの場合に不可能なことは[制限事項](/explanation/limitations/)にあります。

合成の規則:

- テーブルはネストの内部までキーごとに合成します。記述したキーの値が上書き前の値を置換します。
- 形式が種類で決定するテーブル（`kind`で決定する`writer`、`op`で決定するブートの操作、`expect`、スロットの配置の`header`）と事実の値は、記述すると全体を置換します。
- 値の配列（文字列や整数の配列）は、記述すると全体を置換します。
テーブルの配列は後述の表のとおりです。キーで合成する配列では、上書きのエントリは、キーが同一の既存のエントリのうち、記述した欄だけを変更します。新規キーのエントリは、既存のエントリの後に追加します。全体を追加か削除する配列では、既存のエントリの部分的な変更は不可能です。そのエントリを削除して、新規のエントリの全体を記述します（同一の上書き内で可能です）。

順序に意味がある配列（最初の要素が既定の`flash_map`、最初の要素でDAGONが動作する`core`、ブート順の`boot.record.rings`）には、上書きのテーブルに`dagon-order.<array> = [<key>, ...]`を記述して順序を指定可能です。合成した配列のキーを、各1度ずつ列挙します。欠落や重複がある一覧や、存在しないキーを列挙した一覧は拒否します。ネストした配列はその位置を連結して記述します（`dagon-order.boot.record.rings`）。例えば16MiBのフラッシュを搭載したRP2040のボードのプロジェクトは、フラッシュマップを追加して既定にします。

```toml
[mcu_override.rp2040]
dagon-order.flash_map = ["rp2040-16m", "rp2040-4m"]

[[mcu_override.rp2040.flash_map]]
name = "rp2040-16m"
base = 0x1000_0000
bytes = 0x100_0000
xip = true
```

削除: キーで合成する配列と全体を追加か削除する配列のエントリは、キーの欄と`dagon-remove = true`だけを記述すると削除します。任意のテーブルは`dagon-remove = true`だけを記述すると削除します。定義の全体は削除不可です。上書き前の定義にないエントリやテーブルを削除する上書きは、その上書きを表示して拒否します。

欄の解除: エントリやテーブルに`dagon-unset = ["<field>", ...]`を記述すると、上書きの欄の適用前に、その任意の欄を解除します。任意の欄を別の任意の欄に変更する場合に使用します（スロットを`partition`から`at`と`size`へ、バスを`hz`から`baud`へ）。キーの欄、上書き前の定義にない欄、上書き不可の欄は解除不可です。

```toml
[[mcu_override.esp32s3.slot]]
image = "app"
binding = "any"
dagon-unset = ["partition"]
at = 0x11_0000
size = 0x30_0000
```

必須のテーブルの削除や必須の欄の解除は、合成した定義の通常の読込と確認が拒否し、そのエラーは上書きを表示します。`dagon-remove`、`dagon-unset`、`dagon-order`は合成処理だけが読込するキーで、合成した定義には残存しません。

変更不可の欄: `id`、`wire_id`、ボードの`mcu`、`format`、`port`、`partitions`、書込ツールのファイル（`writer.bootloader`、`writer.checksums`）、部品のフォルダ（`part.sub`）は、定義の識別か、定義のフォルダのファイルを表します。そのため変更する上書きはその欄を表示して拒否します。追加する部品で指定可能な`sub`は、上書き前の定義の部品が指定するものだけです。`[[doctor]]`は上書き不可です。別の内容にする場合は、新規idで定義を登録します。

ボードの`[flash]`: 上書きは最初にマイコンとボードの定義のそれぞれに適用します（`[mcu_override]`をマイコンに、`[board_override]`をボードに）。その後に、合成したボードに`[flash]`の`erase_regions`があれば、そのボードのノードでは、その値が合成したマイコンの消去の領域を全体置換します。`[flash]`はテーブルとして合成し、`erase_regions`は記述すると全体を置換します。`flash.part`を変更する上書きは、`[flash]`のテーブルの全体を記述します。消去の時間は部品固有の値であるため、以前の部品の値を継承しないためです。全体を記述しない場合は、その上書きを表示して拒否します。

合成した定義は`dagon.toml`の上書きの適用後、定義の形式の確認と読込に先行して作成します。以後は他の定義と同一の読込と確認を経由します。エラーは`dagon.toml`と上書きのテーブル（`[mcu_override.<id>]`）を表示します。合成した定義はプロジェクトの文字列として処理します（表示する名称の処理と、コマンドの実行前の確認。前述の「このPCの外部に送信するもの」と後述の「検証」）。上書き前の定義のフォルダは無変更のため、ファームウェアと部品は本来の位置で検出可能です。CLI、DAGON Console、ホスト、署名、ファームウェアのビルド（`boot/<id>/`、前述）は、いずれも合成した定義を使用します。

配列と、形式が種類で決定するテーブルの規則と、変更不可の欄です。

<!-- generated from dagon-topology's merge ROWS: python site/scripts/gen.py -->
| 定義 | 位置 | 規則 |
|---|---|---|
| マイコン | `flash_map` | キー（`name`）でエントリごとに合成。`dagon-order`で順番を指定可能 |
| マイコン | `slot` | キー（`image`、`binding`）でエントリごとに合成 |
| マイコン | `part` | キー（`name`）でエントリ全体を追加か削除 |
| マイコン | `doctor` | 上書き不可 |
| マイコン | `peripheral` | キー（`id`）でエントリごとに合成 |
| マイコン | `peripheral.dma_requests` | 記述すると全体を置換 |
| マイコン | `peripheral.privileged_why` | 記述すると全体を置換 |
| マイコン | `core` | キー（`id`）でエントリ全体を追加か削除。`dagon-order`で順番を指定可能 |
| マイコン | `rom_i2c.pairs` | キー（`sda`、`scl`）でエントリごとに合成 |
| マイコン | `link_port` | キー（`name`）でエントリごとに合成 |
| マイコン | `relay_link` | キー（`port`）でエントリごとに合成 |
| マイコン | `relay_link.addressing` | 記述すると全体を置換 |
| マイコン | `boot.record.rings` | キー（`writer`）でエントリごとに合成。`dagon-order`で順番を指定可能 |
| マイコン | `boot.record.fuse.cells` | キー（`image`）でエントリごとに合成 |
| マイコン | `boot.record.entropy.min_bits` | 記述すると全体を置換 |
| マイコン | `isolation.capabilities.idau` | 記述すると全体を置換 |
| マイコン | `isolation.tcm` | 記述すると全体を置換 |
| マイコン | `slot_layout` | キー（`target`、`slot`）でエントリごとに合成 |
| マイコン | `slot_layout.header` | 記述すると全体を置換 |
| マイコン | `execution.erase_regions` | キー（`start`）でエントリごとに合成 |
| マイコン | `execution.erase_regions.unit_max_ms` | 記述すると全体を置換 |
| マイコン | `execution.masked_max_us` | 記述すると全体を置換 |
| マイコン | `watchdog.flag` | 記述すると全体を置換 |
| マイコン | `writer` | 記述すると全体を置換 |
| マイコン | `stage0.clock` | 記述すると全体を置換 |
| マイコン | `bootcore.clock` | 記述すると全体を置換 |
| マイコン | `app.clock` | 記述すると全体を置換 |
| マイコン | `id` | 変更不可（定義の識別情報か、パッケージのファイル） |
| マイコン | `wire_id` | 変更不可（定義の識別情報か、パッケージのファイル） |
| マイコン | `format` | 変更不可（定義の識別情報か、パッケージのファイル） |
| マイコン | `port` | 変更不可（定義の識別情報か、パッケージのファイル） |
| マイコン | `partitions` | 変更不可（定義の識別情報か、パッケージのファイル） |
| マイコン | `writer.bootloader` | 変更不可（定義の識別情報か、パッケージのファイル） |
| マイコン | `writer.checksums` | 変更不可（定義の識別情報か、パッケージのファイル） |
| マイコン | `part.sub` | 変更不可（定義の識別情報か、パッケージのファイル） |
| ボード | `header` | キー（`pin`）でエントリごとに合成 |
| ボード | `i2c` | キー（`sda`、`scl`）でエントリごとに合成 |
| ボード | `part` | キー（`name`）でエントリ全体を追加か削除 |
| ボード | `pin` | キー（`pin`）でエントリごとに合成 |
| ボード | `bus` | キー（`id`）でエントリごとに合成 |
| ボード | `peripheral` | キー（`id`）でエントリごとに合成 |
| ボード | `device` | キー（`id`）でエントリごとに合成 |
| ボード | `device.regs` | キー（`name`）でエントリごとに合成 |
| ボード | `device.writable` | 記述すると全体を置換 |
| ボード | `boot.action` | 記述すると全体を置換 |
| ボード | `boot.action.ops` | 記述すると全体を置換 |
| ボード | `flash` | キーごとに合成。`part`の変更時はテーブル全体を記述 |
| ボード | `flash.erase_regions` | 記述すると全体を置換 |
| ボード | `clock.source` | 記述すると全体を置換 |
| ボード | `id` | 変更不可（定義の識別情報か、パッケージのファイル） |
| ボード | `mcu` | 変更不可（定義の識別情報か、パッケージのファイル） |
| ボード | `format` | 変更不可（定義の識別情報か、パッケージのファイル） |
| ボード | `part.sub` | 変更不可（定義の識別情報か、パッケージのファイル） |
<!-- end of generated table -->

## 検証

定義の数値は全部その数値を読込するエンジンの要件と照合して検証します。エンジンはその要件を`dagon-topology/src/defs/requirements.rs`で宣言します（UF2の`sector`: 256以上の2のべき乗。ログの`face_size`: 96バイト以上。スロットのサイズがヘッダを超過すること。全リージョンが全フラッシュマップの範囲内にあり、重複しないことなど）。要件を宣言していない数値の欄があると、テストが失敗します。

読込時には以下の項目も検証します。

- 未知の欄の不在
- マイコンのid、ワイヤのid、フラッシュマップの名称、ボードのidの一意性
- 前述の上書きの表でキーがある配列のエントリについて、キーの一意性
- ボードのマイコンの存在
- 全スロット、分離したヘッダ、レイアウトのリージョンが、マイコンの全フラッシュマップの範囲内にあり、重複しないこと
- イメージとバインディングごとに、スロットが1つであること
- 分離したヘッダが、パーティションの範囲内にあること
- 参照するファイルが相対パスで、定義のフォルダの範囲内にあり、（ディスクから読込する場合は）存在すること
- 部品のフォルダ（`sub`）にも同一の条件を適用し、部品の`name` / `bin`が単純な名称であること

32ビットを超過するアドレスはエラーです。その場で定義する`[board.<id>]`のボードには定義のフォルダがないため、部品の列挙は不可能です。プロジェクトの定義にあるDoctorの`fix`と書込のコマンドは、利用者が明示的に確認しない限り実行しません。
