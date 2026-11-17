---
title: "マイコンとボードの定義"
description: "dagon-mcu.tomlとdagon-board.toml: DAGONがマイコンやボードについて知っていることです。"
---

マイコンの定義（`targets/mcu/<id>/`）はそれぞれ`dagon-mcu.toml`を持ち、ボードの定義（`targets/boards/<id>/`）はそれぞれ`dagon-board.toml`を持ちます。ここに、DAGONがマイコンやボードについて知っていることを書きます。DAGONのコードが持つのは、種類の閉じた集合と、それを使うエンジンだけです。`dagon-topology::defs`がこれらを読み込んで検証し、`Registry`にします。

- `Registry::builtin()`: このツリーのすべての`targets/mcu/*/dagon-mcu.toml`と`targets/boards/*/dagon-board.toml`。フォルダから見つけ、ビルド時に埋め込みます（ファイルシステムを使いません。ブラウザ向けのビルドはこちらを使います）。定義のフォルダを足すと定義が増えます。フォルダの順に意味はありません。
- `Registry::load(&[Source::Tree(root), Source::Project(dagon_toml)])`: DAGONのソースツリーの定義が組み込みの定義に代わり、プロジェクトは自分の定義を足します。プロジェクトの定義は、すでにあるマイコンやボードのidを使えません。プロジェクトは、すでにある定義の変える欄だけを上書きできます（下の「プロジェクトの定義」）。

数値はTOMLの整数です（`0x0800_8000`、`400_000`）。知らない欄はエラーです。種類の欄は、下に挙げる値だけを受け付けます。

## dagon-mcu.toml

<!-- generated from dagon-topology's McuDef: python site/scripts/gen.py -->
| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `format` | 整数 | 任意 | 定義の形式番号（`2`。省くと1です）。その番号を読まないdagonは、ファイルを受け付けません（site/src/content/docs/explanation/versioning.md）。 |
| `id` | 文字列 | 必須 | 構成で使うマイコンのid（`stm32f401re`）。 |
| `name` | 文字列 | 必須 | 表示名（`STM32F401RE`）。 |
| `wire_id` | 整数 | 必須 | イメージのヘッダの`mcu_id`。重複しないこと。 |
| `port` | 文字列 | 必須 | `self-boot`か`vendor-stage2`。 |
| `gpio_count` | 整数 | 任意 | ボードの無いノードで確かめるGPIO0..n-1。ピンをポートで名付けるときは0。 |
| `[[flash_map]]` | テーブルの配列 | 必須 | `name`（重複しないこと）、`base`（フラッシュのバイト0のアドレス）、`bytes`、`xip`（必須。`true`: CPUがフラッシュを`base`で読むので、フラッシュはRAMやレジスタブロックとアドレス空間を共有します。`false`: `base`とスロットは、MMUが別の場所に割り当てるフラッシュのオフセットです）。最初のものが既定です。 |
| `partitions` | 文字列 | 任意 | 定義のフォルダの中のESP-IDFのパーティションテーブルのCSV（行にはオフセットと大きさの明示が必要です）。 |
| `[layout]` | テーブル | 任意 | 任意の固定のリージョン: `stage0 = { at, size }`、`bootcore_log` / `app_log = { faces = [a, b], face_size }`。`bootcore_log`が`app_log`と同じときは、ブートローダとAPPの両方の記録を持つ1つのログです。 |
| `[stage0]` | テーブル | 任意 | マイコンのDAGONのstage0。`[layout]`に`stage0`があるときだけ書きます: `key`（stage0がブートローダの署名を確かめる鍵を読む場所。既定で唯一の場所は`"flash"`で、stage0をビルドするときにstage0自身のフラッシュに書き込む定数です。ほかの場所は拒みます）と`region_bytes`（領域の全体で、`[layout] stage0`の大きさと同じです。stage0のイメージはその1/16を空け、リンクがそれを確かめます）。`[[stage0.clock]]`はstage0が動くクロックの設定を並べます: `{ source = "internal", out_hz }`（リセットから動くマイコン自身の発振器）をちょうど1つと、`{ source = "crystal" \| "external", in_hz, out_hz, pll = { ... } }`を任意の数。`pll`はマイコン固有の項目を持ち（RP2040: `refdiv`、`fbdiv`、`postdiv1`、`postdiv2`で、水晶から。STM32F401: `m`、`n`、`p`、`q`、`flash_wait_states`で、水晶または外部クロックから）、その項目が示すPLLの規則で確かめます。DAGONが知るどのPLLも示さない項目の`pll`は拒みます。ノードのstage0は、ボードの`[clock]`の供給元（同じ種類と`in_hz`）に合う設定のうち`out_hz`が最も高いもので動き、合うものがなければinternalの設定で動きます。種類と`in_hz`が同じで`out_hz`も同じ設定が2つあれば拒みます。 |
| `[bootcore]` | テーブル | 任意 | DAGONのブートローダのクロック。マイコンに`bootcore`のスロットがあるときだけ書きます: `[[bootcore.clock]]`は、`[[stage0.clock]]`がstage0にするのと同じように、ブートローダが動く設定を並べ、同じPLLの規則で確かめます。ブートローダのHALが選んだ設定を組みます。internalの設定は、マイコン自身の発振器からPLLを動かせます: `{ source = "internal", in_hz, out_hz, pll = { ... } }`で、`in_hz`はその発振器の周波数です（STM32F401のHSI）。`pll`がなければ`out_hz`だけを書きます。ノードのブートローダは、stage0と同じく、ボードの`[clock]`が選ぶ設定で動き、合うものがなければinternalの設定で動きます。 |
| `[app]` | テーブル | 任意 | アプリのクロック。マイコンに`bootcore`のスロットがあるときだけ書きます: `[[app.clock]]`は、アプリについての`[[bootcore.clock]]`です。選んだ設定は、`dagon gen`がノードの`wiring.rs`に書き、ボードの定義は自分の定義のモジュールに書きます。マイコンの定義のアプリの起動の処理がそれを組みます。RP2040では、USBのPLL（`usb_refdiv`、`usb_fbdiv`、`usb_postdiv1`、`usb_postdiv2`で、48MHzを作ります）を持つ設定が、アプリにUSBのクロックを与えます。持たない設定のアプリは、USBの上りのリンクがないときだけビルドできます。internalの設定には`tolerance_ppm`（発振器が`out_hz`からずれうる幅）を書けます。これの無いinternalの設定でアプリが動くノードは、そのクロックでUARTリンクを動かしません（V-26）。 |
| `[[slot]]` | テーブルの配列 | 必須 | `image`（`app`、`bootcore`、`agent`、`stage0`）、`binding`（`a`、`b`、`any`）、`header`（`inline`、`trailer`、`detached`）。領域は`at` + `size`か`partition`です。分離したヘッダは`header_at`か`header_partition` + `header_offset`にあります。 |
| `[writers]` | テーブル | 必須 | マイコンのファームウェアが何を書き込めるか: `[[slot]]`が名付けるイメージの種類ごとに、それを書き込む種類を並べます（`app = ["app"]`、`bootcore = ["app"]`）。並べる種類はどれも、ここにスロットを持つ種類です。ある種類は、自分のスロットから動いて、別の種類のスロットに書き込みます。自分を並べた種類は、自分が動いていない自分のスロットに書き込みます。これはその種類のスロットが2つのときだけ数えます。種類ごとのスロットは1つ（`a`）か2つ（`a`と`b`）です。スロットが1つの種類は、自分以外の種類が書き込むときだけ読み込めます。 |
| `[writer]` | テーブル | 必須 | 最初の書き込みのツール: `kind`とそのパラメータ（下記）。 |
| `[[part]]` | テーブルの配列 | 任意 | 最初の書き込みのために、この定義のフォルダからビルドするDAGONの部品: `name`、`sub`（定義のフォルダの中のフォルダ）、`bin`。 |
| `[coordinator]` | テーブル | 任意 | 親機になれるマイコンだけ: `pio_state_machines`、`pio_uart_state_machines`（リンクごと）、`uart0_tx`、`uart0_rx`。構成がそのマイコンを親機にできるのは、`[coordinator]`が定義されているときだけです（V-25）。 |
| `[link_uart]` | テーブル | 任意 | マイコンの定義のコードがDAGONのUARTリンクを動かすUARTのビットレートの作り方。HALが計算するとおりです: そのクロックを`clocks_per_bit`と、`frac_bits`ビットの小数部を持つ分周比の積で割り、切り捨てます。クロックは`clock_hz`（どのクロックの設定でも変わらない周波数）か`clock = "app"`（ノードの`[[app.clock]]`の設定のコアのクロック。V-26はノードごとにこれを取ります。`tolerance_ppm`の無いinternalの設定で動くノードは、UARTリンクを動かしません）のどちらか1つです。`bauds`は、そのコードがリンクを動かすビットレートの一覧です。構成の確認（V-26）は、どちらかの端の一覧に無いか、どちらかの端で2 %より大きくずれるリンクの`baud`を受け付けません。DAGON Consoleは両端の一覧にある速さだけを選べるようにします。`[links]`に`uart`があるマイコンでは必須です。これが無いと、そのマイコンのファームウェアが動かすUARTリンクはすべて受け付けません。動くと分かっている速度が無いためです。 |
| `[link_i2c]` | テーブル | 任意 | マイコンの定義のファームウェアが、コントローラとしてもターゲットとしても、DAGONのI2Cリンクを動かすバスの速度の一覧: `hzs`。構成の確認（V-20）は、バス上のどれかのノードの一覧に無いバスの`speed_hz`を受け付けません。DAGON Consoleはバス上のすべてのノードの一覧にある速さだけを選べるようにします。`[links]`に`i2c`があるマイコンでは必須です。これが無いと、そのマイコンのファームウェアがつながるI2Cバスはすべて受け付けません。動くと分かっている速度が無いためです。 |
| `[rom_i2c]` | テーブル | 任意 | ROMのI2Cブートローダ（`stm32-i2c-rom`）: `name`（確認のメッセージでの呼び名）、`addr`、`max_hz`、`erase_sectors`、待ち受ける`pairs = [{ controller, sda, scl }]`。 |
| `[links]` | テーブル | 任意 | マイコンの定義のファームウェアが実装するリンクの種類（その`LINK_KINDS`）。 |
| `[boot_receive]` | テーブル | 任意 | アプリを書き込むブート段階（`[writers] app`が`app`以外の種類を示すとき）が、アプリが応答しないときにどう受け取るか（省くと、どのリンクの種類も待ち受けの時間に加わらず、ブート段階は書き込みモードか復旧モードのときだけ受け取ります）: `listen_window_ms`（0から200。省くと50。0は待ち受けの時間を開きません）は、起動してからブートするまでSTAYを待つ時間です。`kinds = { <link kind> = true \| false, ... }`は、リンクの種類ごとに待ち受けの時間に加わるかどうかです（書かない種類は加わりません）。相手が先に列挙する種類（`usb-cdc`）は加われません。ブート段階は`[links]`の種類のすべてのリンクで受け取り、どれかが上ということはありません。加わる種類では起動してすぐから、ほかの種類では書き込みモードか復旧モードで受け取ります。プロジェクトは`[mcu_override.<id>.boot_receive]`で待ち受けの時間を変えます。 |
| `[[link_port]]` | テーブルの配列 | 任意 | 構成のリンクの端が名前で示す、マイコンのリンクのインタフェース: `index`（0から255）、`name`（`a-z`、`0-9`、`-`）、`kind`（リンクの種類: `i2c`、`uart`、`can`、`ethernet`、`usb-cdc`、`ble`、`wifi-tcp`）。`index`と`name`はそれぞれ重なりません。リンクの端の`port`は、リンクの種類のこれらの名前のどれかです（構成の確認V-27）。コンパイル済みの構成はその`index`を持ちます。 |
| `[[relay_link]]` | テーブルの配列 | 任意 | このマイコンのノードが中継するリンク。`[[link_port]]`ごとに1つまでです: `id`（RELAYのリンクの番号。1から254で、重なりません）、`port`（`[[link_port]]`の名前）、`addressing`（`"seven-bit"`: 構成が届く側の端に書くアドレス。アドレスのある種類で使います。`{ fixed = N }`（Nは0から255）: アドレスのない種類の、ただ1つの相手）、`max_payload`（1から、そのポートの種類のプロトコルの最大まで）。マイコンごとのファームウェアのビルドがこれを`RELAY_LINKS`として生成し、PCも同じ項目を読みます。リレーはリンクごとに、`max_payload`の1フレーム分（`RELAY_LINK_BUFFERS`）のバッファだけを持ちます。そのバッファにリンクの種類の最大のフレームが入らないときは、ビルドが止まります。バッファより長いデータ（RELAYの中のフレーム、I2C_RAWで書くデータと読む長さ）は`E_ARG`で断り、切り詰めません。I2C_RAWの上限は、復旧に使うリンクのバッファです。 |
| `[relay]` | テーブル | 任意 | 中継するアプリのファームウェアの予算: `ram_bytes`（静的なRAM、`.bss`と`.data`の合計のバイト数。1から、マイコンのRAMの大きさまで）。`[[relay_link]]`のあるマイコンだけが書きます。CIはマイコンのパッケージの`examples/relay.rs`をビルドして測り、この値を超えると失敗にします（`scripts/relay_ram.py`）。 |
| `[connection]` | テーブル | 必須 | 書き込みのときに利用者がつなぐ方法: `label`、`hint`。 |
| `[[doctor]]` | テーブルの配列 | 任意 | ツールの確認: `what`、`probe`（コマンド）、`expect`（`"runs"`、`{ line = "..." }`、`{ prefix = "..." }`）、`fix`（コマンド）、`hint`、`slow`（`true`: `dagon doctor --fix`が、インストールに時間がかかることを始める前に伝えます）。 |
| `[ram]` | テーブル | 任意 | `{ at, size }`: マイコンのRAMのすべて。イメージは、ブート記録、ヒューズのセル、拡張のリージョンより下のRAMを得ます。 |
| `[isolation]` | テーブル | 任意 | DAGONが拡張を自分から遠ざける方法（下記）。省くと`kind = "none"`、`level = "none"`です。 |
| `[[peripheral]]` | テーブルの配列 | 任意 | レジスタブロックごと: `id`、`base`、`size`、`window_eligible`（拡張がウィンドウとして得られる）、`bus_master`（メモリに書ける）、`privileged_why`（`dma`、`flash`、`watchdog`、`clock`、`reset`、`irq-routing`、`pinmux`、`power`、`rom`、`core-control`、`protection`、`shared`の1つか一覧）、`dma = { streams, channels }`（DMAコントローラ、つまりバスマスタ: そのストリーム。それぞれ`channels`個の要求の1つを選びます）、`dma_requests = [{ signal, controller, stream, channel }]`（マイコンのDMA要求の割り当て表のうち、このブロックの行: `signal`は`a-z 0-9`で`tx`、`rx`、`ch1`、`up`など。1つのセルは1つのブロックに使います）。ボードの定義とマイコンのテンプレートは、ここからDMAのストリームを取ります（取られていないストリームの、その要求の最初の行）。 |
| `[[core]]` | テーブルの配列 | 任意 | プロセッサのコアごと、`id`。DAGONは最初のコアで動きます。 |
| `[watchdog]` | テーブル | 任意 | `kind`（`iwdg`、`rp-watchdog`、`rtc-wdt`）、`trial_ms`（試行のブートで有効化）、`confirmed_ms`（それ以外で有効化）、`trial_attempts`（ブート段階がロールバックの前に待機中のイメージに与えるブートの回数: ブートローダの上限か、ベンダーの第2段階の上限）。アプリが再起動を遅らせられるのは、短いほうの周期の半分までです。マイコンの定義のビルドが、`trial_attempts`をブートローダとエージェントが使う定数にします。`flag`（必須）: ロックアップヒューズに要るウォッチドッグによるリセットのフラグ（ウォッチドッグによるリセットで立ち、電源投入以外のそれ以降のリセットでも残り、起動が確認すると消える）で、`none`、`register`（リセットのフラグのレジスタ。名前と出典をコメントで示します）、`package`（マイコンの定義のコードが保持）のどれかの事実です: `"register"`、`{ unverified = "package" }`、`"unmeasured"`。出所のあるフラグが無いマイコンには、ロックアップヒューズはありません。`range_ms`（必須）: `{ min, max }`。そのマイコンのウォッチドッグが動かせる周期（ms）で、出典をコメントで示します。`trial_ms`か`confirmed_ms`がその外にある定義は、定義の読み込みのときと、ファームウェアの配置を生成するときに受け付けません。 |
| `[execution]` | テーブル | 任意 | 実行モデルのハードウェアの事実（site/src/content/docs/en/explanation/lifecycle-events.mdの「Execution model」）: `masked_max_us`（DAGON自身のコードが割り込みを止める最長の時間、1..=1000）、`flash_op`（消去や書き込みが何を止めるか: `fetch-stall`、`xip-off-masked`、`cache-off-masked`）、`erase_regions = [{ start, len, unit, unit_max_ms }]`（DAGONが消去するフラッシュ: 領域ごとの消去の単位と、その単位の消去にかかるデータシートの最大の時間。単位の整数倍で、重ならないこと。`unit_max_ms`の2倍が`watchdog.trial_ms`と`watchdog.confirmed_ms`以下であること）。事実とは、出所に照らして確かめた数値（コメントで出典を示します）、`{ unverified = N }`、または`"unmeasured"`（値が無い。`unit_max_ms`には使えません）です。 |
| `[[slot_layout]]` | テーブルの配列 | 任意 | ノードが受け取るXFERの対象とスロットのすべて: `target`（`app`、`bootcore`、`agent`。取り除いた`topology`は受け付けません）、`slot`（`a`、`b`、`factory`）、`start`、`capacity`（ヘッダを除いた本体の最大の大きさ）、`header`（`start`にある`"inline"`か、`{ detached = <address> }`）、`partition`（任意: 本体の領域にあたるパーティション。パーティションの表と照らします）。対象とスロットごとに1つで、`[execution]`と一緒に書きます。`[[slot]]`のイメージはそれぞれ自分のものを持ち、最大の本体の消去は`erase_regions`の中に収まります。ノードもホストも、すべての消去をここから計画します。 |
| `[conformance]` | テーブル | 任意 | `scratch = { at, size }`: 適合試験のビルドが消去して書くフラッシュ。どのスロット、ヘッダ、ログの面、stage0からも離します。 |
| `[boot]` | テーブル | 任意 | `[boot.record]`、DAGONのブート段階を持つマイコン用: ブート記録とロックアップヒューズの場所。`kind`（`ram`の中の`noinit-ram`、`[[peripheral]]`の中の`retained-registers`）、`at`、`size`（最大4096）、`survives`（`reset-random`、`reset-cleared`、`power`: 電断の後に残るもの）、`entropy = { source, min_bits }`（必須。新しい記録のシーケンスが混ぜる起動時のワード: `source`は`rng`、`ring-osc`、`osc-capture`、`adc-noise`のどれか。`min_bits`はワードあたりの最小エントロピーで、事実です。値があるなら16以上）、`rings = [{ writer, at, size }]`（ブート段階ごとに1つ、ブートの順に、記録の中）、`fuse = { cells = [{ image, at, size }] }`（`bootcore`、`app`。記録の外）、`reserved`（記録が決してストアしないアドレス）。中のバイトのレイアウトは`boot-record`の取り決めのものです。 |
| `boot_capabilities` | 文字列の配列 | 任意 | ボードのブートのアクションプログラムが、ピンのほかに使えるもの: `["i2c"]`で`bus_write`を使えます。 |
<!-- end of generated table -->

スロットの配置: `inline`は512バイトのヘッダをスロットの先頭に置き、その後にペイロードを置きます。`trailer`はペイロードをスロットの先頭に置き、ヘッダを最後の512バイトに置きます。`detached`はヘッダを別の場所に置き、ペイロードがスロットを埋めます。`any`のスロットはスロットAとして答えます。

スロットの数: イメージの種類ごとに、`[[slot]]`が並べる1つ（`a`）か2つ（`a`と`b`）です。`a`の無い`b`は読み込みません。2つなら、新しいイメージはその種類が動いていない方のスロットに入り、前のバージョンはもう一方に残ります。1つのスロットを読み込むのは、`[writers]`がその種類のほかに書き込む種類を並べるときだけです。イメージは自分が動いているスロットを書き直せないからです。そうでない定義は読み込まず、その種類と、このマイコンでほかにそれを書き込めるイメージが無いことを伝えます。書き込む種類もそれぞれ、ここにスロットを持ちます。1つのスロットでできないこと（前のバージョン、書き込み中の電断、復旧するイメージ）は[制限事項](/explanation/limitations/)にあります。

書き込みの種類:

- `uf2-drive`: `family`、`sector`、`info_file`、`info_header`、`board_id`（ブートROMのドライブは、情報ファイルが`info_header`で始まり、`Board-ID: <board_id>`の行を持つものです）。
- `probe-rs`: `chip`。
- `espflash`: `chip`、`flash_size`、`table_at`、`bootloader`、`checksums`、`usb_hint = { vid, pid }`。ヒントは、書き込みツールが報告する候補にラベルを付けるだけで、候補を絞りません。一致する候補を先頭に並べ、あらかじめ選んでおきます。

### 消去と試行の待ち時間

`[execution] erase_regions`は、DAGONが消去するフラッシュを示します。領域（`start`、`len`）ごとに`unit`バイトの単位で丸ごと消去し、フラッシュの操作1回で1単位を消去し、1単位にかかる時間は最大で`unit_max_ms`です。アドレスによって単位が違うマイコン（STM32F401の16、64、128KiBのセクタ）は、単位の大きさごとに1つの領域を挙げます。フラッシュがボードの部品であるマイコン（RP2040とESP32-S3）では、マイコンの領域は、候補のどの部品も持つ消去を使う既定の値です。4KiBのセクタで、1つにかかる時間はそれらの部品のセクタの消去の最大の時間のうち、いちばん長いものです。決まった部品を載せるボードは、それを`[flash]`に書きます（`part`、コメントでデータシートの出典を示し、同じ形の`erase_regions`）。その領域は、そのボードのノードではマイコンの領域をまるごと置き換え、マイコンのウォッチドッグとスロットの配置に照らして確かめます。`[[slot_layout]]`は、ノードが受け取る対象とスロットのすべてを、本体の始まり、最大の本体、ヘッダの置き場所（先頭の`"inline"`か、ESP32-S3がdagonパーティションにヘッダを置くような`{ detached = <address> }`）とともに挙げます。

ノードの実際の領域（ボードに`[flash]`があればそれ、無ければマイコンのもの）とマイコンのスロットの配置から、1つの計画の関数（`dagon_port::timeout`）が転送の消去を決めます。転送が覆うバイト（スロットの先頭からのinlineのヘッダと本体、またはdetachedのヘッダと本体を別々に）と、それを含む領域の単位の1つ1つです。ノードのエージェントは、その計画だけをスロットのストアに渡します。ストアは領域をボードから受け取ります（同じ定義からボードの定義のcrateかノードのwiring.rsに生成される`ERASE_REGIONS`で、どの`BoardDef`も必ず返します）。ホストと適合試験は、同じ定義から同じ転送を計画し、ノードが消去するリクエストを、計画した単位の`unit_max_ms`の合計に1000msを足した時間だけ待ちます。ノードの試行の待ち時間は`[watchdog] trial_ms`と`trial_attempts`から来ます。ホストは1回の試行の中で新しいバージョンのHELLOを確かめてCONFIRMを送り、新しいバージョンを待つ時間は`trial_attempts`回の試行を覆います（マイコンごとの値は、その待ち時間を作るときにホストの待ち時間と照らして確かめます）。

マイコンの定義が無いノード、または`[watchdog]`の無い定義のノードには、ホストと適合試験は既定の値を使います。消去の待ち時間は30000ms、試行は8000msで、3回です。`[execution]`の無い定義のノードでは、消去の待ち時間が既定の30000msです。

### `[isolation]`

<!-- generated from dagon-topology's Isolation: python site/scripts/gen.py -->
| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `level` | 文字列 | 任意 | DAGONが強制するもの: `none`、`mpu`、`pms`、`tz`。種類は`none`か1つのレベルを与えます（MPUとPMPの種類は`mpu`、`esp-pms`は`pms`、`armv8m-tz`は`tz`）。 |
| `kind` | 文字列 | 任意 | 保護のハードウェア: `armv6m-mpu`、`armv7m-mpu`、`armv8m-mpu`、`armv8m-tz`、`riscv-pmp`、`riscv-epmp`、`esp-pms`、`none`。 |
| `boot_extensions` | 真偽値 | 任意 | マイコンが、拡張を動かすDAGONのブート段階を持ちます（`port = "self-boot"`のときだけ）。 |
| `ext_ram_max` | 整数 | 任意 | 1つのイメージのサンドボックスの拡張が取るRAMの上限。リージョンは切り上げます（`kind`が`none`でないときは必須）。 |
| `root_protection` | 文字列の配列 | 任意 | `dagon provision`が設定できるルートイメージの保護: `rdp1`（元に戻せる）、`rdp2`（永久）、`wrp`（元に戻せる）、`esp-secure-boot`、`esp-flash-encryption`（永久）。 |
| `cache` | 文字列 | 任意 | `none`、`i-cache`、`id-cache`（命令とデータ）。 |
| `[[isolation.tcm]]` | テーブルの配列 | 任意 | 密結合メモリ、`[{ at, size }]`。 |
| `[isolation.capabilities]` | テーブル | 任意 | その種類の能力の記録。ブート時にハードウェアから読み返します（一致しないとレベルが`none`になります）。`armv6m-mpu`: `mpu_regions`は0か8、`min_region`は256以上、`subregions_from`。`armv7m-mpu`: `mpu_regions`は8か16、`min_region`は32以上、`subregions_from`。`armv8m-mpu`: `mpu_regions`は0、4、8、12、16。`armv8m-tz`: `sau_regions`（0、4、8）、`s_mpu_regions`、`ns_mpu_regions`、`idau = [{ base, size, attribution }]`。`riscv-pmp`、`riscv-epmp`: `pmp_entries`（0、16、64）、`grain`、`na4`、`tor`。`esp-pms`: `worlds`、`gdma_channels`、`memory_regions`、`peripheral_slots`。どの種類も: `critical_section`（armv6mは`primask`、armv7mとarmv8mは`basepri`、RISC-Vは`mstatus-mie`、esp-pmsは`xtensa-level`）、`unwind`（種類ごとに`cortex-m-exception`、`armv8m-tz`、`riscv-trap`、`xtensa-world`）、`timer`（`systick`、`riscv-mtime`、`timer-group`）、`reset_cleanup`（`peripheral-reset`、`reinit`）、`background_privileged`、`fault_address`。任意で`np_max_us`と`dma_stop_max_us`。これらはDAGONの上限（50と20）を下げることだけができます。 |
<!-- end of generated table -->

`kind`が`none`でない定義は、すべてのコアと、`flash`、`watchdog`、`clock`、`reset`、`irq-routing`の各区分のブロックを少なくとも1つ挙げます（コアが複数あるときは`core-control`も、`esp-pms`と`riscv-epmp`では`protection`も）。バスマスタは決してウィンドウにできず、ウィンドウにできないブロックはすべて理由を示します。区分が欠けた定義は、その区分の名前を示して受け付けません。

## dagon-board.toml

<!-- generated from dagon-topology's BoardDef: python site/scripts/gen.py -->
| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `format` | 整数 | 任意 | 定義の形式番号（`2`。省くと1です）。その番号を読まないdagonは、ファイルを受け付けません（site/src/content/docs/explanation/versioning.md）。 |
| `id` | 文字列 | 必須 | `nodes[].board`で使うボードのid。 |
| `product` | 文字列 | 必須 | 製品名。 |
| `mcu` | 文字列 | 必須 | マイコンのid（定義されていること）。 |
| `flash_bytes` | 整数 | 必須 | フラッシュの大きさ（マイコンのフラッシュマップのどれかが収まること）。 |
| `i2c_target` | 真偽値 | 任意 | ボードがI2Cのターゲットになれるか。 |
| `drawing` | 文字列 | 必須 | UIの図。 |
| `default` | 真偽値 | 任意 | `true`: マイコンだけを指定したノードを確かめるときのボード（既定のボードがすでにある組み込みのマイコンの既定を、プロジェクトは置き換えられません。印の付いたボードが2つあると読み込みません）。マイコンごとに、こう指定できるボードは最大1つです。マイコンの唯一のボードは、これが無くても既定です。ボードが複数で印が無いマイコンには既定がありません（そのノードはボードの事実なしで確かめます）。定義の順で決まることはありません。 |
| `[[header]]` | テーブルの配列 | 任意 | `[{ pin, label }]`。ボードで確かめていない`label`は省きます。構成がこのボードで名指しできるピンのすべてです（親機側の`pins`・`control`・バスの`sda`/`scl`。ノード側の`node_pins`は`i2c`から選びます）。構成の確認（V-07）はそれ以外を受け付けず、DAGON Consoleはこれだけを選べるようにします。 |
| `[[i2c]]` | テーブルの配列 | 任意 | ヘッダから使える`[{ controller, sda, scl }]`の組。 |
| `[default_link]` | テーブル | 任意 | 構成が別に指定しないときの、ノードのボードの配線: `kind`（`i2c`、`uart`、`can`、`ethernet`）、`iface`（`hw-uart0`、`pio-uart`）、`addr`、`baud`、`control`（`full`、`cooperative`）、`rom_entry`（`force-download-boot`、`m5pm1`、`stm32-i2c-rom`）。親機では省きます。 |
| `[coordinator]` | テーブル | 任意 | このボードが親機のときに、ページが新しいリンクを配線するピン: `i2c = { controller, sda, scl }`（新しいI2Cバス。`controller`はボードの`i2c`のどれか）と`pio_uart = { tx, rx }`（新しいPIO UARTリンク）。マイコンの`gpio_count`より小さい番号のGPIOです。マイコンに`[coordinator]`があるボードだけです。 |
| `[[part]]` | テーブルの配列 | 任意 | ボードの定義自身からビルドする部品（`name`、`sub`、`bin`）。 |
| `[boot]` | テーブル | 任意 | ブート段階のボードの値: `led`（`[[pin]]`）、`led_active`（`high`、`low`）、`halt = { pattern, period_ms }`（`blink`、`code`、`off`。DAGONのstage0があるマイコンで`led`があるときは、stage0が周期の10分の1ずつでコードを点滅させるため、`blink`の周期は10 ms以上です）、`power_hold = { pin, level }`。ブートのアクションプログラムは`[[boot.action]]`です。ブートのアクションプログラム（`boot-program`形式1）: `on`（イベントのカタログがアクションに使えると示すブートイベント）と`ops`: `{ op = "led", state }`（`on`、`off`、`toggle`）、`{ op = "gpio", pin, level }`（`boot = true`の`[[pin]]`。電源保持のピンは保持のレベルだけ）、`{ op = "delay", ms }`（1から20）、`{ op = "bus_write", device, reg, bytes }`（`writable`の一覧にある`[[device]]`のレジスタ、1から8バイト。`boot_capabilities = ["i2c"]`のマイコンだけ）。イベントごとに最大32の操作と50 msで、バスの書き込みは5 msと数えます。 |
| `[[pin]]` | テーブルの配列 | 任意 | ボードのピン: `pin`、`name`（ボードの定義のコードが取るときの名前、`a-z 0-9 _`）、`boot`（ブートプログラムが動かせる）、`levels`（許すレベル。省くとすべて）。 |
| `[[bus]]` | テーブルの配列 | 任意 | ボードのバス: `id`（`a-z 0-9 - _`）、`kind`（`i2c`、`spi`、`uart`、`can`）、`controller`（マイコンの`[[peripheral]]`）、信号ごとのピン（i2cは`sda`、`scl`。uartは`tx`と`rx`の両方かどちらか。canは`tx`、`rx`。spiは`sck`と、`mosi`と`miso`の両方かどちらか）、`hz`（i2c、spi）または`baud`（uart、can。`dagon = true`のUARTバスでは、dagonが構成の速度を渡さないビルドのリンクの速度。後述）、`dagon`（`true`: DAGON自身のリンクがここで動くので、バスもそのコントローラも拡張のものにはなりません）、`dma`（`true`: ボードの定義のコードはバスをDMAで開くか渡します。i2c、spi、uart。txとrxのストリームは、マイコンの`dma_requests`からバスの順に取り、ボードの定義のビルドが`<Bus>TxDma`/`<Bus>RxDma`、`take_dma!`、`bind_<bus>_dma!`として書き出します）。 |
| `[[peripheral]]` | テーブルの配列 | 任意 | 拡張が宣言できるマイコンのブロックの、ボードでの名前: `id`、`mcu`（マイコンの`[[peripheral]]`のid。`dagon = true`のバスのコントローラは指定できません）。 |
| `[[device]]` | テーブルの配列 | 任意 | ボードのバス上のデバイス: `id`（`a-z 0-9 - _`）、`bus`、`addr`、`max_transfer`（必須: 1回の転送で受け取れるデータの最大バイト数。データシートから取ります。バスのポートとブートプログラムはこれを守ります）、`regs = [{ name, reg }]`（名前付きのレジスタ、`a-z 0-9 _`）、`writable`（DAGONの外のものが書いてよいレジスタ: `regs`の番号か名前）。 |
| `[flash]` | テーブル | 任意 | フラッシュがボードの部品であるマイコンで、ボードが載せるフラッシュの部品: `part`（コメントでデータシートの出典を示します）と、マイコンの`[execution]` `erase_regions`と同じ形の`erase_regions`（`start`、`len`、`unit`、`unit_max_ms`）。このボードのノードでは、マイコンの領域をまるごと置き換えます。マイコンのウォッチドッグとスロットの配置に照らして確かめます。省くと、マイコンの領域です。ノードもホストも、同じ領域からすべての消去を計画します。 |
| `[clock]` | テーブル | 任意 | このボードでマイコンのクロック入力を駆動するもの: `source`は`"internal"`、`{ crystal_hz = N }`、`{ external_hz = N }`のどれかです（Nの出典の回路図かデータシートをコメントに書きます）。省くとinternalです。stage0、DAGONのブートローダ、アプリは、それぞれマイコンのこの供給元に合う設定（`[[stage0.clock]]`、`[[bootcore.clock]]`、`[[app.clock]]`）で動き、合うものがなければinternalの設定で動きます。それらの段階のないマイコンでは使いません。 |
<!-- end of generated table -->

ボードの定義は、DAGONのUARTリンクを、構成のリンクの`baud`で動かします。`dagon build`、`deploy`、`flash`は、ノードのすべてのビルド（アプリと復旧用のエージェント）に、そのノードのUARTリンクの速度を`DAGON_LINK_BAUD`として渡すので、両端が同じ値でビルドされます。これが無いビルド（単なる`cargo build`）は、`[default_link] baud`か、ボードの`dagon = true`のUARTバスの`baud`を使います。

`[[pin]]`とバス全体で、同じピンは2度現れません。コードの生成元になる名前（ピンの名前、バスのidとその信号、デバイスのid。`-`は`_`として読みます）も、それぞれ1度だけ現れます。バスを宣言するボードでは、`[coordinator]`のピンは、`dagon = true`のI2CとUARTのバスのピンです。

ボードの定義のビルドスクリプトは、ボードの定義を読み（`dagon_topology::defs::codegen::package_build`）、結果を取り込みます。`DAGON_BOOT_CRATE`があるときに読むのは、そのフォルダの`board.toml`（`dagon gen`が書く、プロジェクトの上書きを当てたボードの定義。下記）で、無いときはソースツリーのボードの定義です。ボードのid、フラッシュの大きさ、既定のリンク、ブートのLEDと停止のパターン、バスの速度とデバイスを定数として（`def.rs`）、名前の付いたピン、バス、バスのピンを`take!(p.<name>)`マクロ付きのHALの型として（`pins.rs`）、各デバイスを`dagon_events::Device`のバスのポートの能力として（`devices.rs`。作る方法はこれだけです）。検証で読んだ定義のどれかが変わると、ビルドをもう一度実行します。マイコンの定義のブートイメージは、ボードの`[boot]`を同じように読みます（`boot_build`）。読むのは、ビルドの対象のノードのボードです。`DAGON_BOOT_CRATE`がノードのブート用crate（プロジェクトの`boot/<id>/`。`dagon gen`が書きます）を示し、その`Cargo.toml`がノード、マイコン、ボード（このツリーのボードか、プロジェクトの`[board.<id>]`）、信頼されたブート拡張を示します。これが無いビルドは失敗します。ボードの無いノードには、ブートのLEDも停止のパターンもありません。コードは自分ではピンもアドレスもパターンも持ちません。

マイコンの定義のビルドスクリプトは、ノードのビルドに渡されたマイコンの定義から配置を生成します（`dagon_mcu_build::definition`）。イメージの種類ごとのすべてのスロットのアドレスと長さ、stage0の領域、ログ、消去の領域とスロットの配置、ウォッチドッグと試行の値です。その定義は`boot/<id>/mcu.toml`で、`dagon gen`がすべてのノードについて書きます。中身は、組み込みの定義にプロジェクトの上書きを当てたものです。ボードのあるノードには、同じように上書きを当てたボードの定義`board.toml`も書きます。さらに`dagon gen`は、書いたファイルごとに1行を`defs.sha256`に書きます。行は、ファイルのSHA-256（小文字の16進数64桁）、空白2つ、ファイル名で、UTF-8、改行はLFです。ノードの定義を読むビルド（マイコンとボードのビルドスクリプト、`dagon build`、`deploy`、`flash`）は、行と合わなくなったファイルや、`defs.sha256`の無いフォルダを受け付けません。ノードの定義を変えるときは、`dagon.toml`の上書きを直して、`dagon gen`をもう一度実行します。アプリとブート用crateの`.cargo/config.toml`は、どちらも`DAGON_BOOT_CRATE`をそのフォルダにします。`dagon build`、`deploy`、`flash`も、ノードのためにビルドするすべてのイメージに、そのフォルダを渡します（ボードの部品、たとえば復旧用のエージェントも含みます）。そのため、1つのノードのすべてのイメージは同じファイルからビルドされます。スロットを動かす、スロットごとに長さを変える、ウォッチドッグの周期を変えるプロジェクトは、コードを直さずにそれに合ったファームウェアを得ます。別のマイコンや別の定義の形式の`mcu.toml`は受け付けません。`DAGON_BOOT_CRATE`の無いビルド（パッケージの単なる`cargo build`）は、パッケージ自身の`dagon-mcu.toml`を読みます。

## このPCの外に出るもの

定義ファイルは秘密を書く場所ではありません。定義の中の名前（id、製品名、ピンのラベル、パーティション名）はDAGON Consoleに表示され、Claudeに頼んだときはClaudeにも送られます。コマンド、ヒント、コメント、ファイルのパスは送りません。プロジェクト自身の名前は、40文字以内の`A-Z a-z 0-9`・空白・`.`・`_`・`-`だけのときにそのまま出し、それ以外は`project-text`と表示します。

## プロジェクトの定義

プロジェクトの`dagon.toml`は、先頭に`format = 1`を書きます。これは`dagon.toml`の形式の番号です（[取り決めと部品](/reference/contracts/)の`project`）。`format`の無いファイルや別の番号のファイルは、このdagonが読む番号を示して受け付けません。`dagon.toml`の最上位のキーは次のものだけで、ほかのキーは受け付けません。

<!-- generated from dagon-topology's project KEYS: python site/scripts/gen.py -->
`format`、`topology`、`release`、`provision_dir`、`pubkey`、`signing_key`、`provision`、`build`、`dagon`、`extensions`、`board`、`mcu_files`、`mcu_override`、`board_override`
<!-- end of generated table -->

`dagon.toml`は、ボードを`[board.<id>]`のテーブルとしてその場で定義でき（`id`を除いたボードのスキーマ）、マイコンのファイルを`mcu_files = ["defs/my-mcu/dagon-mcu.toml"]`で登録できます。定義が参照するファイルは、その定義のフォルダの中で解決します。

`[board.<id>]`の`<id>`はボードのidで、ノードの`board:`がこのidを指します。`dagon.toml`の`[board.<id>]`は、いつもボードの定義です。そのボードにはボードのcrateが無いので、そのボードを指すノードは、`dagon gen`がマイコンのcrateから作り、ボードの`flash_bytes`を`wiring.rs`に書きます。マイコンだけを指すノードにはボードがいりません。ボードもフラッシュの大きさも求めず、DAGON Consoleは、マイコンの定義の既定のフラッシュマップが配置するフラッシュの大きさを、読み取り専用で示します。

### 定義の上書き

すでにある定義（組み込みの定義か、DAGONのソースツリーの定義）の一部を変えるときは、定義を写さずに、変える欄だけを`dagon.toml`に書きます。`[mcu_override.<id>]`はマイコンの定義を、`[board_override.<id>]`はボードの定義を上書きします。中身はその定義と同じスキーマで、変える欄だけを持ちます。上書きはプロジェクト全体で、idごとです。そのマイコンやボードを使うすべてのノードが、上書きを当てた定義を使います。書かない欄は元の定義のままなので、DAGONの新しいリリースで元の定義が変わると、その欄も変わります。一部のノードだけが違う定義を使うときは、新しいidで定義を登録します（`mcu_files`、`[board.<id>]`）。プロジェクト自身の定義は上書きせず、その定義を直します。無いidの上書きは受け付けません。

例: RP2040のブートローダのスロットを1つにして、空いた128KiBをアプリのスロットAに足します。`dagon.toml`に次を足します。

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

スロットはキー（`image`と`binding`）で、受け取りの配置はキー（`target`と`slot`）で、元のエントリを見つけます。アプリのスロットAで変わるのは書いた欄だけで、`header`などほかの欄は元の定義のままです。ブートローダのスロットが1つでも読み込めるのは、`[writers]`でアプリがブートローダを書き込むからです。1つのスロットでできないことは[制限事項](/explanation/limitations/)にあります。

合成の規則:

- テーブルは、入れ子の中までキーごとに合成します。書いたキーの値が元の値に代わります。
- 形が種類で決まるテーブル（`kind`で決まる`writer`、`op`で決まるブートの操作、`expect`、スロットの配置の`header`）と事実の値は、書くと丸ごと置き換わります。
- 値の配列（文字列や整数の配列）は、書くと丸ごと置き換わります。
- テーブルの配列は、下の表のとおりです。キーで合成する配列では、上書きのエントリは、キーが同じ元のエントリの、書いた欄だけを変えます。新しいキーのエントリは、元のエントリの後に足します。丸ごと足すか消す配列では、すでにあるエントリの一部を変えられません。そのエントリを消して、新しいエントリを丸ごと書きます（同じ上書きの中でできます）。

順に意味のある配列（最初のものが既定の`flash_map`、最初のものでDAGONが動く`core`、ブートの順の`boot.record.rings`）は、上書きのテーブルに`dagon-order.<array> = [<key>, ...]`を書いて順を与えられます。合成した配列のキーを、ちょうど1度ずつ並べます。抜けや重なりのある一覧や、無いキーを並べた一覧は受け付けません。入れ子の配列は、その場所をつないで書きます（`dagon-order.boot.record.rings`）。たとえば、16MiBのフラッシュを載せたRP2040のボードのプロジェクトは、フラッシュマップを足して既定にします。

```toml
[mcu_override.rp2040]
dagon-order.flash_map = ["rp2040-16m", "rp2040-4m"]

[[mcu_override.rp2040.flash_map]]
name = "rp2040-16m"
base = 0x1000_0000
bytes = 0x100_0000
xip = true
```

消すこと: キーで合成する配列と丸ごと足すか消す配列のエントリは、キーの欄と`dagon-remove = true`だけを書くと消えます。任意のテーブルは、`dagon-remove = true`だけを書くと消えます。定義の全体は消せません。元の定義に無いエントリやテーブルを消す上書きは、上書きを示して受け付けません。

欄を外すこと: エントリやテーブルに`dagon-unset = ["<field>", ...]`を書くと、上書きの欄を当てる前に、その任意の欄を外します。任意の欄を別の任意の欄に替えるときに使います（スロットを`partition`から`at`と`size`へ、バスを`hz`から`baud`へ）。キーの欄、元の定義に無い欄、上書きできない欄は外せません。

```toml
[[mcu_override.esp32s3.slot]]
image = "app"
binding = "any"
dagon-unset = ["partition"]
at = 0x11_0000
size = 0x30_0000
```

必須のテーブルを消すことや必須の欄を外すことは、合成した定義のふつうの読み込みと確認が受け付けず、そのエラーは上書きを示します。`dagon-remove`、`dagon-unset`、`dagon-order`は合成だけが読むキーで、合成した定義には残りません。

変えられない欄: `id`、`wire_id`、ボードの`mcu`、`format`、`port`、`partitions`、書き込みのツールのファイル（`writer.bootloader`、`writer.checksums`）、部品のフォルダ（`part.sub`）は、定義の識別か、定義のフォルダのファイルを示すので、変える上書きはその欄を示して受け付けません。足す部品が名付けられる`sub`は、元の定義の部品が名付けるものだけです。`[[doctor]]`は上書きできません。別のものにするときは、新しいidで定義を登録します。

ボードの`[flash]`: 上書きは、まずマイコンとボードの定義のそれぞれに当てます（`[mcu_override]`をマイコンに、`[board_override]`をボードに）。その後で、合成したボードに`[flash]`の`erase_regions`があれば、そのボードのノードでは、それが合成したマイコンの消去の領域をまるごと置き換えます。`[flash]`はテーブルとして合成し、`erase_regions`は書くと丸ごと置き換わります。`flash.part`を変える上書きは、`[flash]`のテーブルを丸ごと書きます。消去の時間は部品のものなので、前の部品の値を引き継がないためです。丸ごとでなければ、上書きを示して受け付けません。

合成した定義は、`dagon.toml`の上書きを当てた後、定義の形式の確認と読み込みより前に作ります。その後は、ほかの定義と同じ読み込みと確認を通ります。エラーは`dagon.toml`と上書きのテーブル（`[mcu_override.<id>]`）を示します。合成した定義はプロジェクトの文字列として扱います（表示する名前の扱いと、コマンドを実行する前の確認。上の「このPCの外に出るもの」と下の「検証」）。元の定義のフォルダはそのままなので、ファームウェアと部品は元の場所から見つかります。CLI、DAGON Console、ホスト、署名、ファームウェアのビルド（`boot/<id>/`、上記）は、どれも合成した定義を使います。

配列と、形が種類で決まるテーブルの規則と、変えられない欄です。

<!-- generated from dagon-topology's merge ROWS: python site/scripts/gen.py -->
| 定義 | 場所 | 規則 |
|---|---|---|
| マイコン | `flash_map` | キー（`name`）でエントリごとに合成。`dagon-order`で順を与えられる |
| マイコン | `slot` | キー（`image`、`binding`）でエントリごとに合成 |
| マイコン | `part` | キー（`name`）でエントリを丸ごと足すか消す |
| マイコン | `doctor` | 上書きできない |
| マイコン | `peripheral` | キー（`id`）でエントリごとに合成 |
| マイコン | `peripheral.dma_requests` | 書くと丸ごと置き換わる |
| マイコン | `peripheral.privileged_why` | 書くと丸ごと置き換わる |
| マイコン | `core` | キー（`id`）でエントリを丸ごと足すか消す。`dagon-order`で順を与えられる |
| マイコン | `rom_i2c.pairs` | キー（`sda`、`scl`）でエントリごとに合成 |
| マイコン | `link_port` | キー（`name`）でエントリごとに合成 |
| マイコン | `relay_link` | キー（`port`）でエントリごとに合成 |
| マイコン | `relay_link.addressing` | 書くと丸ごと置き換わる |
| マイコン | `boot.record.rings` | キー（`writer`）でエントリごとに合成。`dagon-order`で順を与えられる |
| マイコン | `boot.record.fuse.cells` | キー（`image`）でエントリごとに合成 |
| マイコン | `boot.record.entropy.min_bits` | 書くと丸ごと置き換わる |
| マイコン | `isolation.capabilities.idau` | 書くと丸ごと置き換わる |
| マイコン | `isolation.tcm` | 書くと丸ごと置き換わる |
| マイコン | `slot_layout` | キー（`target`、`slot`）でエントリごとに合成 |
| マイコン | `slot_layout.header` | 書くと丸ごと置き換わる |
| マイコン | `execution.erase_regions` | キー（`start`）でエントリごとに合成 |
| マイコン | `execution.erase_regions.unit_max_ms` | 書くと丸ごと置き換わる |
| マイコン | `execution.masked_max_us` | 書くと丸ごと置き換わる |
| マイコン | `watchdog.flag` | 書くと丸ごと置き換わる |
| マイコン | `writer` | 書くと丸ごと置き換わる |
| マイコン | `stage0.clock` | 書くと丸ごと置き換わる |
| マイコン | `bootcore.clock` | 書くと丸ごと置き換わる |
| マイコン | `app.clock` | 書くと丸ごと置き換わる |
| マイコン | `id` | 変えられない（定義の識別か、パッケージのファイル） |
| マイコン | `wire_id` | 変えられない（定義の識別か、パッケージのファイル） |
| マイコン | `format` | 変えられない（定義の識別か、パッケージのファイル） |
| マイコン | `port` | 変えられない（定義の識別か、パッケージのファイル） |
| マイコン | `partitions` | 変えられない（定義の識別か、パッケージのファイル） |
| マイコン | `writer.bootloader` | 変えられない（定義の識別か、パッケージのファイル） |
| マイコン | `writer.checksums` | 変えられない（定義の識別か、パッケージのファイル） |
| マイコン | `part.sub` | 変えられない（定義の識別か、パッケージのファイル） |
| ボード | `header` | キー（`pin`）でエントリごとに合成 |
| ボード | `i2c` | キー（`sda`、`scl`）でエントリごとに合成 |
| ボード | `part` | キー（`name`）でエントリを丸ごと足すか消す |
| ボード | `pin` | キー（`pin`）でエントリごとに合成 |
| ボード | `bus` | キー（`id`）でエントリごとに合成 |
| ボード | `peripheral` | キー（`id`）でエントリごとに合成 |
| ボード | `device` | キー（`id`）でエントリごとに合成 |
| ボード | `device.regs` | キー（`name`）でエントリごとに合成 |
| ボード | `device.writable` | 書くと丸ごと置き換わる |
| ボード | `boot.action` | 書くと丸ごと置き換わる |
| ボード | `boot.action.ops` | 書くと丸ごと置き換わる |
| ボード | `flash` | キーごとに合成。`part`を変えるときはテーブルを丸ごと書く |
| ボード | `flash.erase_regions` | 書くと丸ごと置き換わる |
| ボード | `clock.source` | 書くと丸ごと置き換わる |
| ボード | `id` | 変えられない（定義の識別か、パッケージのファイル） |
| ボード | `mcu` | 変えられない（定義の識別か、パッケージのファイル） |
| ボード | `format` | 変えられない（定義の識別か、パッケージのファイル） |
| ボード | `part.sub` | 変えられない（定義の識別か、パッケージのファイル） |
<!-- end of generated table -->

## 検証

定義の数値はどれも、それを読むエンジンが必要とするものに照らして確かめます。エンジンはその要件を`dagon-topology/src/defs/requirements.rs`で宣言します（UF2の`sector`: 256以上の2のべき乗。ログの`face_size`: 96バイト以上。スロットはヘッダより大きいこと。どのリージョンもすべてのフラッシュマップの中にあり、重ならないことなど）。要件を宣言していない数値の欄があると、テストが失敗します。

読み込み時には、次のことも確かめます。知らない欄がないこと。マイコンのid、ワイヤのid、フラッシュマップの名前、ボードのidが重複しないこと。上の上書きの表でキーを持つ配列のエントリが、キーを重複させないこと。ボードのマイコンが存在すること。すべてのスロット、分離したヘッダ、レイアウトのリージョンが、マイコンのすべてのフラッシュマップの中にあり、重ならないこと。イメージとバインディングごとにスロットが1つであること。分離したヘッダがパーティションの中にあること。参照するファイルが相対パスで、定義のフォルダの中にとどまり、（ディスクから読み込むときは）存在すること。部品のフォルダ（`sub`）についても同じで、部品の`name` / `bin`が単純な名前であること。32ビットを超えるアドレスはエラーです。その場で定義する`[board.<id>]`のボードには定義のフォルダがないので、部品を挙げられません。プロジェクトの定義にあるDoctorの`fix`と書き込みのコマンドは、利用者がはっきり確認しない限り実行しません。
