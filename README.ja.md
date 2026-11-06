<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/product-symbol/lockup-horizontal-dark-2400w.png">
    <img src="assets/product-symbol/lockup-horizontal-light-2400w.png" alt="DAGON Boot" width="480">
  </picture>
</p>

# DAGON Boot (DIY Assured-update Gateway for Own Nodes)

[English: README.md](README.md)

**動くハードウェアを簡単に作り、作ったあとも育て続ける。**

DAGON Boot は、いくつかのマイコンを組み合わせて何かを作り、棚や壁や腕に置いたあとも良くしていきたい開発者（はじめての方から経験者まで）のためのものです。ボード（またはそのマイコン）を選び、つなぎ方を描き、アプリを Rust で書くだけで、残りは DAGON が受け持ちます。

- **ビルドできるプロジェクト。** `dagon new` と `dagon gen` が、ノードごとに起動・配線・自分のコードを書く空の `app.rs` を持つ Rust の crate を作ります。フラッシュの配置、イメージヘッダ、リンカの番地、ブートローダは DAGON が決めます。
- **全体へ安全に書き込む。** PC の USB につないだ親機が、各ノードへ新しいバージョンを送ります。イメージはすべて署名付きです。ノードは新しいバージョンを試行起動し、確定されなければ自分で前のバージョンに戻ります。応答しなくなったノードも親機経由で書き直せます。
- **同じことができるブラウザのツールとコマンド。** DAGON Console（`dagon serve`）は、ノードとリンクをグラフで見せ、編集のたびに検査し、ビルド・署名・書き込みまで行います。どの手順も `dagon` コマンドでもできます。
- **Claude の手助け。** 新しいプロジェクトには、よくある場面（ビルドが失敗した、一覧に無いボードを使いたい、構成の問題が分からない）のための Claude Code の指示とスキルが入っています。

## ドキュメント

ドキュメントは <https://c-ardinal.github.io/DAGON-Boot/doc/ja/> にあります。
模擬の親機と子機で動く DAGON Console は <https://c-ardinal.github.io/DAGON-Boot/demo/> で試せます。
ページの元は [`site/src/content/docs`](site/src/content/docs) の下のファイルです。

- [DAGON を試す](site/src/content/docs/ja/getting-started/index.md)、[入れる](site/src/content/docs/ja/getting-started/install.md)、
  [はじめから書き込みまで](site/src/content/docs/ja/getting-started/first-project.md)
- やり方: [自分のボード](site/src/content/docs/ja/how-to/own-board.md)、[つながりを差し替える](site/src/content/docs/ja/how-to/swap-link.md)、
  [既にあるファームウェアに組み込む](site/src/content/docs/ja/how-to/existing-firmware.md)、
  [プロジェクトでの Claude の手助け](site/src/content/docs/ja/how-to/claude-help.md)
- しくみ: [このページの言葉](site/src/content/docs/ja/explanation/words.md)、[リポジトリ](site/src/content/docs/ja/explanation/repository.md)、
  [制限事項](site/src/content/docs/ja/explanation/limitations.md)
- リファレンス: [ボードとマイコン](site/src/content/docs/reference/boards.md)、[ライセンス](site/src/content/docs/ja/reference/licenses.md)

## ライセンス

MIT か Apache-2.0 のどちらかを選べます（[LICENSE-MIT](LICENSE-MIT)、[LICENSE-APACHE](LICENSE-APACHE)）。
