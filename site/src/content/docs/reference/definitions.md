---
title: "マイコンとボードの定義"
description: "dagon-mcu.tomlとdagon-board.toml: DAGONがマイコンやボードについて知っていることです。"
---

マイコンの定義（`targets/mcu/<id>/`）はそれぞれ`dagon-mcu.toml`を持ち、ボードの定義（`targets/boards/<id>/`）はそれぞれ`dagon-board.toml`を持ちます。ここに、DAGONがマイコンやボードについて知っていることを書きます。DAGONのコードが持つのは、種類の閉じた集合と、それを使うエンジンだけです。`dagon-topology::defs`がこれらを読み込んで検証し、`Registry`にします。

- `Registry::builtin()`: このツリーのすべての`targets/mcu/*/dagon-mcu.toml`と`targets/boards/*/dagon-board.toml`。フォルダから見つけ、ビルド時に埋め込みます（ファイルシステムを使いません。ブラウザ向けのビルドはこちらを使います）。定義のフォルダを足すと定義が増えます。フォルダの順に意味はありません。
- `Registry::load(&[Source::Tree(root), Source::Project(dagon_toml)])`: DAGONのソースツリーの定義が組み込みの定義に代わり、プロジェクトは自分の定義を足します。プロジェクトの定義は、すでにあるマイコンやボードのidを使えません。

数値はTOMLの整数です（`0x0800_8000`、`400_000`）。知らない欄はエラーです。種類の欄は、下に挙げる値だけを受け付けます。

## dagon-mcu.toml

<!-- generated from dagon-topology's McuDef: python site/scripts/gen.py -->
| キー | 型 | 必須か | 意味 |
|---|---|---|---|
| `format` | 整数 | 任意 | 定義の形式番号（`1`。省くと1です）。その番号を読まないdagonは、ファイルを受け付けません（site/src/content/docs/explanation/versioning.md）。 |
| `id` | 文字列 | 必須 | 構成で使うマイコンのid（`stm32f401re`）。 |
| `name` | 文字列 | 必須 | 表示名（`STM32F401RE`）。 |
| `wire_id` | 整数 | 必須 | イメージのヘッダの`mcu_id`。重複しないこと。 |
| `port` | 文字列 | 必須 | `self-boot`か`vendor-stage2`。 |
| `gpio_count` | 整数 | 任意 | ボードの無いノードで確かめるGPIO0..n-1。ピンをポートで名付けるときは0。 |
| `[[flash_map]]` | テーブルの配列 | 必須 | `name`（重複しないこと）、`base`（フラッシュのバイト0のアドレス）、`bytes`、`xip`（必須。`true`: CPUがフラッシュを`base`で読むので、フラッシュはRAMやレジスタブロックとアドレス空間を共有します。`false`: `base`とスロットは、MMUが別の場所に割り当てるフラッシュのオフセットです）。最初のものが既定です。 |
| `partitions` | 文字列 | 任意 | 定義のフォルダの中のESP-IDFのパーティションテーブルのCSV（行にはオフセットと大きさの明示が必要です）。 |
| `[layout]` | テーブル | 任意 | 任意の固定のリージョン: `stage0 = { at, size }`、`bootcore_log` / `app_log = { faces = [a, b], face_size }`。`bootcore_log`が`app_log`と同じときは、ブートローダとAPPの両方の記録を持つ1つのログです。`stage0_verifies = true`: stage0はブートローダの署名も確かめます（省くとダイジェストだけ）。 |
| `[[slot]]` | テーブルの配列 | 必須 | `image`（`app`、`bootcore`、`agent`、`stage0`）、`binding`（`a`、`b`、`any`）、`header`（`inline`、`trailer`、`detached`）。領域は`at` + `size`か`partition`です。分離したヘッダは`header_at`か`header_partition` + `header_offset`にあります。 |
| `[writer]` | テーブル | 必須 | 最初の書き込みのツール: `kind`とそのパラメータ（下記）。 |
| `[[part]]` | テーブルの配列 | 任意 | 最初の書き込みのために、この定義のフォルダからビルドするDAGONの部品: `name`、`sub`（定義のフォルダの中のフォルダ）、`bin`。 |
| `[coordinator]` | テーブル | 任意 | 親機になれるマイコンだけ: `pio_state_machines`、`pio_uart_state_machines`（リンクごと）、`uart0_tx`、`uart0_rx`。構成がそのマイコンを親機にできるのは、`[coordinator]`が定義されているときだけです（V-25）。 |
| `[link_uart]` | テーブル | 任意 | マイコンの定義のコードがDAGONのUARTリンクを動かすUARTのビットレートの作り方。HALが計算するとおりです: `clock_hz`を`clocks_per_bit`と、`frac_bits`ビットの小数部を持つ分周比の積で割り、切り捨てます。構成の確認（V-26）は、どちらかの端で2 %より大きくずれるリンクの`baud`を受け付けません。`[links]`に`uart`があるマイコンでは必須です。これが無いと、そのマイコンのファームウェアが動かすUARTリンクはすべて受け付けません。動くと分かっている速度が無いためです。 |
| `[rom_i2c]` | テーブル | 任意 | ROMのI2Cブートローダ（`stm32-i2c-rom`）: `name`（確認のメッセージでの呼び名）、`addr`、`max_hz`、`erase_sectors`、待ち受ける`pairs = [{ controller, sda, scl }]`。 |
| `[links]` | テーブル | 任意 | マイコンの定義のファームウェアが実装するリンクの種類（その`LINK_KINDS`）。 |
| `[connection]` | テーブル | 必須 | 書き込みのときに利用者がつなぐ方法: `label`、`hint`。 |
| `[[doctor]]` | テーブルの配列 | 任意 | ツールの確認: `what`、`probe`（コマンド）、`expect`（`"runs"`、`{ line = "..." }`、`{ prefix = "..." }`）、`fix`（コマンド）、`hint`、`slow`（`true`: `dagon doctor --fix`が、インストールに時間がかかることを始める前に伝えます）。 |
| `[ram]` | テーブル | 任意 | `{ at, size }`: マイコンのRAMのすべて。イメージは、ブート記録、ヒューズのセル、拡張のリージョンより下のRAMを得ます。 |
| `[isolation]` | テーブル | 任意 | DAGONが拡張を自分から遠ざける方法（下記）。省くと`kind = "none"`、`level = "none"`です。 |
| `[[peripheral]]` | テーブルの配列 | 任意 | レジスタブロックごと: `id`、`base`、`size`、`window_eligible`（拡張がウィンドウとして得られる）、`bus_master`（メモリに書ける）、`privileged_why`（`dma`、`flash`、`watchdog`、`clock`、`reset`、`irq-routing`、`pinmux`、`power`、`rom`、`core-control`、`protection`、`shared`の1つか一覧）、`dma = { streams, channels }`（DMAコントローラ、つまりバスマスタ: そのストリーム。それぞれ`channels`個の要求の1つを選びます）、`dma_requests = [{ signal, controller, stream, channel }]`（マイコンのDMA要求の割り当て表のうち、このブロックの行: `signal`は`a-z 0-9`で`tx`、`rx`、`ch1`、`up`など。1つのセルは1つのブロックに使います）。ボードの定義とマイコンのテンプレートは、ここからDMAのストリームを取ります（取られていないストリームの、その要求の最初の行）。 |
| `[[core]]` | テーブルの配列 | 任意 | プロセッサのコアごと、`id`。DAGONは最初のコアで動きます。 |
| `[watchdog]` | テーブル | 任意 | `kind`（`iwdg`、`rp-watchdog`、`rtc-wdt`）、`trial_ms`（試行のブートで有効化）、`confirmed_ms`（それ以外で有効化）、`trial_attempts`（ブート段階がロールバックの前に待機中のイメージに与えるブートの回数: ブートローダの上限か、ベンダーの第2段階の上限）。アプリが再起動を遅らせられるのは、短いほうの周期の半分までです。マイコンの定義のビルドが、`trial_attempts`をブートローダとエージェントが使う定数にします。`flag`（必須）: ロックアップヒューズに要るウォッチドッグによるリセットのフラグ（ウォッチドッグによるリセットで立ち、電源投入以外のそれ以降のリセットでも残り、起動が確認すると消える）で、`none`、`register`（リセットのフラグのレジスタ。名前と出典をコメントで示します）、`package`（マイコンの定義のコードが保持）のどれかの事実です: `"register"`、`{ unverified = "package" }`、`"unmeasured"`。出所のあるフラグが無いマイコンには、ロックアップヒューズはありません。 |
| `[execution]` | テーブル | 任意 | 実行モデルのハードウェアの事実（site/src/content/docs/en/explanation/lifecycle-events.mdの「Execution model」）: `masked_max_us`（DAGON自身のコードが割り込みを止める最長の時間、1..=1000）、`flash_op`（消去や書き込みが何を止めるか: `fetch-stall`、`xip-off-masked`、`cache-off-masked`）、`erase_unit`（DAGONが1回の操作で消去する最大の単位、2のべき乗）、`erase_unit_max_ms`（その消去のデータシートの最大値。`watchdog.confirmed_ms`より小さいこと）。事実とは、出所に照らして確かめた数値（コメントで出典を示します）、`{ unverified = N }`、または`"unmeasured"`（値が無い。`erase_unit_max_ms`には使えません）です。 |
| `[conformance]` | テーブル | 任意 | `scratch = { at, size }`: 適合試験のビルドが消去して書くフラッシュ。どのスロット、ヘッダ、ログの面、stage0からも離します。 |
| `[boot]` | テーブル | 任意 | `[boot.record]`、DAGONのブート段階を持つマイコン用: ブート記録とロックアップヒューズの場所。`kind`（`ram`の中の`noinit-ram`、`[[peripheral]]`の中の`retained-registers`）、`at`、`size`（最大4096）、`survives`（`reset-random`、`reset-cleared`、`power`: 電断の後に残るもの）、`entropy = { source, min_bits }`（必須。新しい記録のシーケンスが混ぜる起動時のワード: `source`は`rng`、`ring-osc`、`osc-capture`、`adc-noise`のどれか。`min_bits`はワードあたりの最小エントロピーで、事実です。値があるなら16以上）、`rings = [{ writer, at, size }]`（ブート段階ごとに1つ、ブートの順に、記録の中）、`fuse = { cells = [{ image, at, size }] }`（`bootcore`、`app`。記録の外）、`reserved`（記録が決してストアしないアドレス）。中のバイトのレイアウトは`boot-record`の取り決めのものです。 |
| `boot_capabilities` | 文字列の配列 | 任意 | ボードのブートのアクションプログラムが、ピンのほかに使えるもの: `["i2c"]`で`bus_write`を使えます。 |
<!-- end of generated table -->

スロットの配置: `inline`は512バイトのヘッダをスロットの先頭に置き、その後にペイロードを置きます。`trailer`はペイロードをスロットの先頭に置き、ヘッダを最後の512バイトに置きます。`detached`はヘッダを別の場所に置き、ペイロードがスロットを埋めます。`any`のスロットはスロットAとして答えます。

書き込みの種類:

- `uf2-drive`: `family`、`sector`、`info_file`、`info_header`、`board_id`（ブートROMのドライブは、情報ファイルが`info_header`で始まり、`Board-ID: <board_id>`の行を持つものです）。
- `probe-rs`: `chip`。
- `espflash`: `chip`、`flash_size`、`table_at`、`bootloader`、`checksums`、`usb_hint = { vid, pid }`。ヒントは、書き込みツールが報告する候補にラベルを付けるだけで、候補を絞りません。一致する候補を先頭に並べ、あらかじめ選んでおきます。

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
| `format` | 整数 | 任意 | 定義の形式番号（`1`。省くと1です）。その番号を読まないdagonは、ファイルを受け付けません（site/src/content/docs/explanation/versioning.md）。 |
| `id` | 文字列 | 必須 | `nodes[].board`で使うボードのid。 |
| `product` | 文字列 | 必須 | 製品名。 |
| `mcu` | 文字列 | 必須 | マイコンのid（定義されていること）。 |
| `flash_bytes` | 整数 | 必須 | フラッシュの大きさ（マイコンのフラッシュマップのどれかが収まること）。 |
| `i2c_target` | 真偽値 | 任意 | ボードがI2Cのターゲットになれるか。 |
| `drawing` | 文字列 | 必須 | UIの図。 |
| `default` | 真偽値 | 任意 | `true`: マイコンだけを指定したノードを確かめるときのボード（既定のボードがすでにある組み込みのマイコンの既定を、プロジェクトは置き換えられません。印の付いたボードが2つあると読み込みません）。マイコンごとに、こう指定できるボードは最大1つです。マイコンの唯一のボードは、これが無くても既定です。ボードが複数で印が無いマイコンには既定がありません（そのノードはボードの事実なしで確かめます）。定義の順で決まることはありません。 |
| `[[header]]` | テーブルの配列 | 任意 | `[{ pin, label }]`。ボードで確かめていない`label`は省きます。 |
| `[[i2c]]` | テーブルの配列 | 任意 | ヘッダから使える`[{ controller, sda, scl }]`の組。 |
| `[default_link]` | テーブル | 任意 | 構成が別に指定しないときの、ノードのボードの配線: `kind`（`i2c`、`uart`、`can`、`ethernet`）、`iface`（`hw-uart0`、`pio-uart`）、`addr`、`baud`、`control`（`full`、`cooperative`）、`rom_entry`（`force-download-boot`、`m5pm1`、`stm32-i2c-rom`）。親機では省きます。 |
| `[coordinator]` | テーブル | 任意 | このボードが親機のときに、ページが新しいリンクを配線するピン: `i2c = { controller, sda, scl }`（新しいI2Cバス。`controller`はボードの`i2c`のどれか）と`pio_uart = { tx, rx }`（新しいPIO UARTリンク）。マイコンの`gpio_count`より小さい番号のGPIOです。マイコンに`[coordinator]`があるボードだけです。 |
| `[[part]]` | テーブルの配列 | 任意 | ボードの定義自身からビルドする部品（`name`、`sub`、`bin`）。 |
| `[boot]` | テーブル | 任意 | ブート段階のボードの値: `led`（`[[pin]]`）、`led_active`（`high`、`low`）、`halt = { pattern, period_ms }`（`blink`、`code`、`off`）、`power_hold = { pin, level }`。ブートのアクションプログラムは`[[boot.action]]`です。ブートのアクションプログラム（`boot-program`形式1）: `on`（イベントのカタログがアクションに使えると示すブートイベント）と`ops`: `{ op = "led", state }`（`on`、`off`、`toggle`）、`{ op = "gpio", pin, level }`（`boot = true`の`[[pin]]`。電源保持のピンは保持のレベルだけ）、`{ op = "delay", ms }`（1から20）、`{ op = "bus_write", device, reg, bytes }`（`writable`の一覧にある`[[device]]`のレジスタ、1から8バイト。`boot_capabilities = ["i2c"]`のマイコンだけ）。イベントごとに最大32の操作と50 msで、バスの書き込みは5 msと数えます。 |
| `[[pin]]` | テーブルの配列 | 任意 | ボードのピン: `pin`、`name`（ボードの定義のコードが取るときの名前、`a-z 0-9 _`）、`boot`（ブートプログラムが動かせる）、`levels`（許すレベル。省くとすべて）。 |
| `[[bus]]` | テーブルの配列 | 任意 | ボードのバス: `id`（`a-z 0-9 - _`）、`kind`（`i2c`、`spi`、`uart`、`can`）、`controller`（マイコンの`[[peripheral]]`）、信号ごとのピン（i2cは`sda`、`scl`。uartは`tx`と`rx`の両方かどちらか。canは`tx`、`rx`。spiは`sck`と、`mosi`と`miso`の両方かどちらか）、`hz`（i2c、spi）または`baud`（uart、can。`dagon = true`のUARTバスでは、dagonが構成の速度を渡さないビルドのリンクの速度。後述）、`dagon`（`true`: DAGON自身のリンクがここで動くので、バスもそのコントローラも拡張のものにはなりません）、`dma`（`true`: ボードの定義のコードはバスをDMAで開くか渡します。i2c、spi、uart。txとrxのストリームは、マイコンの`dma_requests`からバスの順に取り、ボードの定義のビルドが`<Bus>TxDma`/`<Bus>RxDma`、`take_dma!`、`bind_<bus>_dma!`として書き出します）。 |
| `[[peripheral]]` | テーブルの配列 | 任意 | 拡張が宣言できるマイコンのブロックの、ボードでの名前: `id`、`mcu`（マイコンの`[[peripheral]]`のid。`dagon = true`のバスのコントローラは指定できません）。 |
| `[[device]]` | テーブルの配列 | 任意 | ボードのバス上のデバイス: `id`（`a-z 0-9 - _`）、`bus`、`addr`、`max_transfer`（必須: 1回の転送で受け取れるデータの最大バイト数。データシートから取ります。バスのポートとブートプログラムはこれを守ります）、`regs = [{ name, reg }]`（名前付きのレジスタ、`a-z 0-9 _`）、`writable`（DAGONの外のものが書いてよいレジスタ: `regs`の番号か名前）。 |
<!-- end of generated table -->

ボードの定義は、DAGONのUARTリンクを、構成のリンクの`baud`で動かします。`dagon build`、`deploy`、`flash`は、ノードのすべてのビルド（アプリと復旧用のエージェント）に、そのノードのUARTリンクの速度を`DAGON_LINK_BAUD`として渡すので、両端が同じ値でビルドされます。これが無いビルド（単なる`cargo build`）は、`[default_link] baud`か、ボードの`dagon = true`のUARTバスの`baud`を使います。

`[[pin]]`とバス全体で、同じピンは2度現れません。コードの生成元になる名前（ピンの名前、バスのidとその信号、デバイスのid。`-`は`_`として読みます）も、それぞれ1度だけ現れます。バスを宣言するボードでは、`[coordinator]`のピンは、`dagon = true`のI2CとUARTのバスのピンです。

ボードの定義のビルドスクリプトは、自分の定義を読み（`dagon_topology::defs::codegen::package_build`）、結果を取り込みます。ボードのid、フラッシュの大きさ、既定のリンク、ブートのLEDと停止のパターン、バスの速度とデバイスを定数として（`def.rs`）、名前の付いたピン、バス、バスのピンを`take!(p.<name>)`マクロ付きのHALの型として（`pins.rs`）、各デバイスを`dagon_events::Device`のバスのポートの能力として（`devices.rs`。作る方法はこれだけです）。検証で読んだ定義のどれかが変わると、ビルドをもう一度実行します。マイコンの定義のブートイメージは、ボードの`[boot]`を同じように読みます（`boot_build`）。読むのは、ビルドの対象のノードのボードです。`DAGON_BOOT_CRATE`がノードのブート用crate（プロジェクトの`boot/<id>/`。`dagon gen`が書きます）を示し、その`Cargo.toml`がノード、マイコン、ボード（このツリーのボードか、プロジェクトの`[board.<id>]`）、信頼されたブート拡張を示します。これが無いビルドは失敗します。ボードの無いノードには、ブートのLEDも停止のパターンもありません。コードは自分ではピンもアドレスもパターンも持ちません。

## このPCの外に出るもの

定義ファイルは秘密を書く場所ではありません。定義の中の名前（id、製品名、ピンのラベル、パーティション名）はDAGON Consoleに表示され、Claudeに頼んだときはClaudeにも送られます。コマンド、ヒント、コメント、ファイルのパスは送りません。プロジェクト自身の名前は、40文字以内の`A-Z a-z 0-9`・空白・`.`・`_`・`-`だけのときにそのまま出し、それ以外は`project-text`と表示します。

## プロジェクトの定義

プロジェクトの`dagon.toml`は、ボードを`[board.<id>]`のテーブルとしてその場で定義でき（`id`を除いたボードのスキーマ）、マイコンのファイルを`mcu_files = ["defs/my-mcu/dagon-mcu.toml"]`で登録できます。定義が参照するファイルは、その定義のフォルダの中で解決します。

## 検証

定義の数値はどれも、それを読むエンジンが必要とするものに照らして確かめます。エンジンはその要件を`dagon-topology/src/defs/requirements.rs`で宣言します（UF2の`sector`: 256以上の2のべき乗。ログの`face_size`: 96バイト以上。スロットはヘッダより大きいこと。どのリージョンもすべてのフラッシュマップの中にあり、重ならないことなど）。要件を宣言していない数値の欄があると、テストが失敗します。

読み込み時には、次のことも確かめます。知らない欄がないこと。マイコンのid、ワイヤのid、フラッシュマップの名前、ボードのidが重複しないこと。ボードのマイコンが存在すること。すべてのスロット、分離したヘッダ、レイアウトのリージョンが、マイコンのすべてのフラッシュマップの中にあり、重ならないこと。イメージとバインディングごとにスロットが1つであること。分離したヘッダがパーティションの中にあること。参照するファイルが相対パスで、定義のフォルダの中にとどまり、（ディスクから読み込むときは）存在すること。部品のフォルダ（`sub`）についても同じで、部品の`name` / `bin`が単純な名前であること。32ビットを超えるアドレスはエラーです。その場で定義する`[board.<id>]`のボードには定義のフォルダがないので、部品を挙げられません。プロジェクトの定義にあるDoctorの`fix`と書き込みのコマンドは、利用者がはっきり確認しない限り実行しません。
