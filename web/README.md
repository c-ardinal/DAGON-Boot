# web

DAGON Console, the browser tool of DAGON Boot: Svelte 5 + Vite + TypeScript, built for Chrome / Edge 89 and later
(the first versions with Web Serial). Screen text is Japanese only and lives in `src/app/copy.ts`.

The page is local: `dagon serve` hands it out on 127.0.0.1 from the developer's own PC and is
the only place it reads or writes anything (the project's 構成 YAML, releases, boot cores, jobs).
The page fetches nothing from any other origin and offers no download-to-save (releases stay in
the project's `out/`). It opens on the start screen (プロジェクト作成 / サンプルから作成 / プロジェクトのオープン) until
serve has a project open.

The one build of its own is the public demo (`npm run build:demo`, Vite mode `demo`, flag
`src/app/site.ts`): the same page on the simulator (`?sim` below) compiled into the
browser, with the repository's test-key release and F401 boot core as its files. It never calls
serve, reads or writes no files, opens no device and holds no keys; the controls for those
(プロジェクトのオープン, テンプレート生成, ビルド, 書込 over USB, リリース, DAGONへの質問 / 連携, choosing a package file)
stay visible, dimmed, and give the reason in the lock table's `site` row on hover, focus and
press. Every other build compiles the flag to false and leaves the demo's parts out.

構成 edits go to the project's YAML by themselves (PUT /v1/topology about 500 ms after the last
edit; 保存 saves at once). Until a save goes through the band shows 「● 未保存」 and why; 書込 and
運用 wait for the saved 構成, and leaving the project (menu, リスタート, closing the
tab) saves first and asks when it cannot.

## Commands

Needs the `wasm32-unknown-unknown` Rust target (listed in `rust-toolchain.toml`) and `wasm-pack`
0.15.1 on `PATH`; `wasm-pack` fetches the matching `wasm-bindgen` and `wasm-opt` itself.

```sh
npm install
npm run wasm      # build crates/dagon-wasm into src/wasm/pkg (needs DAGON_PUBKEY; see Release key)
npm run dev       # http://127.0.0.1:5173
npm run build     # svelte-check, then vite build into dist/ (target chrome89)
npm run build:demo -- --outDir <dir> --emptyOutDir   # the public demo (simulator, test key)
npm run preview   # serves dist/ on http://127.0.0.1:4173
npm test          # Vitest
npm run e2e       # the Console in a real browser: the layout sweep over window sizes (npx playwright install chromium first)
npm run fonts     # re-fetch the font subsets after screen text changes
```

Query parameters for development (`npm run dev` only; production builds leave the stand-in node out):

| Parameter | Effect |
|---|---|
| `?mock` | Uses an in-memory node connected to the PC instead of Web Serial: the sample coordinator, the 7-segment node on I2C 0x42 and the watch on UART, all on 1.0.0. It answers HELLO, STATUS, RELAY to the two nodes and the update steps (XFER, SET_PENDING, REBOOT, CONFIRM, SHOW), so finding, sending, monitoring and updating run through `dagon-host` unchanged. Pick `crates/dagon-sim/fixtures/sample-1.1.0.dgb` as the update file. |
| `?mock=silent` | In-memory device that never answers (no-response path). |
| `&quiet=uart,i2c:66` | With `?mock`: these nodes do not answer RELAY (not-answering path after two monitoring rounds; `quiet=uart,i2c:66` is the node connected to the PC alone). |
| `&away` | With `?mock`: after its REBOOT the node connected to the PC stays away until chosen again (the reconnect dialog, after the 45 s automatic reopening). |
| `&factory=uart` | With `?mock`: these nodes answer as their factory agent (recovery takes the factory path). |
| `?sim` | The simulator (`crates/dagon-sim` through dagon-wasm's `Simulator`, `sim` feature): the coordinator, F401 with its ROM loader, watch. It runs on the wall clock and each answer takes the links' and flash's time (dagon-sim's `TimedRelay`), so a write runs about as long as on hardware. `&wipe` erases the F401 first, so "7セグへの書込" takes the ROM path. |

With `?mock` and `?sim` (no serve behind the dev server) the repository's test-key release
(`crates/dagon-sim/fixtures/sample-full-1.1.0.dgb`, the apps at their real size) and F401 boot core
(`crates/dagon-wasm/fixtures/f401-bootcore-1.0.0.img`) stand in for the project's files that serve
hands out (GET /v1/release/latest, GET /v1/bootcore/{node}), and the start screen's 空白から作成 / サンプルから作成
start an unsaved 構成 on the page. Production builds leave every stand-in out.

A simulated node connected to the PC plugs in as one more `Transport` in `src/serial/devHub.ts`, under its own parameter.

## Layout

| Path | Contents |
|---|---|
| `src/serial/frame.ts` | DAGON link v1 frame field names and split; encoding, CRC and the stream cutter are `crates/dagon-proto` through dagon-wasm (tests compare against `crates/dagon-format/golden/frame_*.bin`). |
| `src/serial/transport.ts` | `Transport` (open / close / write frame / read frame with timeout) and the Web Serial implementation. |
| `src/serial/hello.ts` | HELLO payload types and the response encoder the mock uses. |
| `src/serial/mock.ts` | In-memory node connected to the PC (see `?mock`) in the same frame format. |
| `src/serial/devHub.ts` | Picks the stand-in node connected to the PC from the page URL. |
| `src/serial/simHub.ts` | The simulator as a `Transport` and clock (builds with the `sim` feature). |
| `src/serial/hostLink.ts` | The byte link `dagon-host` runs on, over a `Transport`; the device under it can be replaced (the node connected to the PC chosen again after its reboot). |
| `src/wasm/dagon.ts` | The only entry to `crates/dagon-wasm`: frame encoding and stream cutting, HELLO request / response, version text, board table, topology check and compile, release verification. `src/wasm/pkg/` is build output and not committed. |
| `src/app/machine.ts` | Operating-mode state machine: states, main and side actions, transitions (`machine.test.ts` checks one main action at most and no dead ends). |
| `src/app/operateLink.svelte.ts` | The operating-mode session's link: connect, find, send, monitoring every `watch_interval_ms()` and the state machine's moves, all through `dagon-host` in `dagon-wasm`. |
| `src/app/operate.svelte.ts` | `Operate`, built on that link: release check and update (`apply`, reconnecting the node connected to the PC, confirm rule), one node's write (`writeNode`: re-transfer, factory, ROM path after the question) and going back (`rollback`). |
| `src/app/writeProgress.ts` | What the page knows about each node in an update: its answers, its write's stage and one %, the chosen release's verdict. |
| `src/app/console/` | The page's own parts `App.svelte` wires: the open project (`workspace`), build, write, linking DAGON's agent, the lock table, notices, each mode's view, the menu and the keys. |
| `src/app/band.ts` | Bottom band per state: cause chip, sentence, named buttons. |
| `src/app/sample.ts` | The sample 構成 (`topology/sample.yaml` plus names and wiring). |
| `src/app/connect.ts` | Device chooser and error mapping. |
| `src/app/copy.ts` | All screen and announcement text. |
| `src/graph/` | The graph: own SVG (wire bundles, ties: `bundles.ts`) + HTML (node cards: `NodeCard.svelte`, chips, ports) under a view only pan, zoom and fit (`fit.ts`) change; links are damped cables (`cable.ts`); roving focus with arrow keys. |
| `src/app/start.svelte.ts` | The start screen: making and opening projects through serve (GET /v1/project, GET /v1/fs, POST /v1/project/open, /new). |
| `src/app/define/` | 準備: the document and its history, the check's problems, the 構成 differences of a release (otherTopology), and the serve link (`cli.svelte.ts`: passphrase, API, events, saving the YAML). |
| `src/screens/` | Header, menu popover and its restart dialog, start screen, bottom band, the columns over the canvas (focal number, parts, detail, zoom, notices), the reconnect dialog of the node connected to the PC; `define/` holds 準備's columns. |
| `src/styles/tokens.css` | Design tokens: colour, type, spacing, radius, z-index, motion. |

`Transport` carries whole frames: the page owns choosing and opening the device, and the protocol
steps belong to `dagon-host`, which will receive a `Transport` through `dagon-wasm`.

## Window sizes

The principle: at every window size the page supports, every piece of content and every control can be reached.
Nothing is lost off the window's side, nothing covers another part, and text keeps its size. The graph's canvas and
the data table are the only parts that may pan or scroll inside themselves, because their layout is two-dimensional
by nature (WCAG 1.4.10). The page holds this at a 1280 × 720 window zoomed to 400 % (320 × 256).

The page meets the principle in two tiers. The boundary between them is 1024 px of window width:

- **1024 px and wider:** the canvas fills the window under the bar, the columns float over it, and the page never
  scrolls. `e2e/layout.spec.ts` sweeps the viewport matrix from 1024 to 2560 px wide.
- **Under 1024 px** (a 1280 window at 200 % zoom and more): the narrow tier. The bar wraps onto more rows, and the
  brand keeps only its mark. The canvas keeps a height of its own (`--h-canvas-narrow`). The columns, the DAGON mark
  and its bubble, and the drawer stack under the canvas, and the page scrolls down. Menus and dialogs fit the window's
  width. The CSS media queries and `src/app/narrow.ts` use the same width. The narrow test in `e2e/layout.spec.ts`
  checks 320 × 256, 640 × 360, 640 × 720 and 1023 × 700 in every state.

## Release key

Keys never reach the browser; only the public release key does. At run time the page verifies
releases (and `apply` verifies them) with the key `dagon serve` gives it: the
`<meta name="dagon-release-key">` serve writes into the page, then `release_key` in GET /v1/hello.
When serve switches projects (the start screen's プロジェクトのオープン / プロジェクト作成) the page reads hello again and takes that
project's key. A page served by serve checks with no key until one has arrived
(`require_release_key`).

`dagon-wasm` is still built with a key: set `DAGON_PUBKEY` to the `.pub` file from `dagon key gen`
(or its 64 hex digits) before `npm run wasm` / `build` / `dev`:

```sh
DAGON_PUBKEY=../keys/dagon.pub npm run build
```

Without it (and without `DAGON_ALLOW_TEST_KEY=1`) `npm run wasm` and `npm run build` stop. `npm test` and `npm run dev` build with
`DAGON_ALLOW_TEST_KEY=1` and the `sim` feature (`npm run wasm:dev`): when `DAGON_PUBKEY` is unset
they use the public RFC 8032 test key, which the stand-in nodes' release (`crates/dagon-sim/fixtures`)
is signed with, and the build warns. `npm run build` leaves the simulator out.

## Web Serial on http://127.0.0.1

The Web Serial API is exposed only in secure contexts. Loopback origins (`http://127.0.0.1`,
`http://localhost`) count as potentially trustworthy, so `dagon serve` pages (and `vite preview` in
development) on 127.0.0.1 get `navigator.serial` without HTTPS. Checked in headless Edge 156 against
`vite preview`: `isSecureContext` is `true`, `navigator.serial` exists and the `serial` permissions
policy allows it. Choosing and opening a real device needs a person in front of the browser.

## npm dependencies

All are development dependencies; nothing from npm except the Svelte runtime ships in the bundle.
The page also fetches `dagon_wasm_bg.wasm` (about 400 KB, 160 KB gzip) in parallel with the first
paint; connecting waits for it, and if it does not load the bottom band offers reloading the page.

| Package | Version | Licence |
|---|---|---|
| svelte | 5.57 | MIT |
| vite | 8.3 | MIT |
| @sveltejs/vite-plugin-svelte | 7.3 | MIT |
| vitest | 5.0 | MIT |
| svelte-check | 4.7 | MIT |
| typescript | 6.0 | Apache-2.0 |

## Fonts

Self-hosted from `public/fonts/` (no third-party CDN at run time). Subsets come from the Google Fonts
API (`npm run fonts`); the licence texts sit next to the files. None of the four declares a Reserved
Font Name, so subsetting and redistribution are allowed under the SIL Open Font License 1.1.

| Font | Upstream | Licence | Weights | Subset | Size |
|---|---|---|---|---|---|
| Doto | github.com/oliverlalan/Doto | OFL 1.1 (`OFL-Doto.txt`) | 700 | digits, `%`, `/`, space | 1.0 KB |
| DotGothic16 | github.com/fontworks-fonts/DotGothic16 | OFL 1.1 (`OFL-DotGothic16.txt`) | 400 | digits, Latin letters, `.`, `/`, space | 3.3 KB |
| Zen Kaku Gothic New | github.com/googlefonts/zen-kakugothic | OFL 1.1 (`OFL-ZenKakuGothicNew.txt`) | 400, 500, 700 | ASCII + every non-ASCII character in `src/` | 3 × 39 KB |
| JetBrains Mono | github.com/JetBrains/JetBrainsMono | OFL 1.1 (`OFL-JetBrainsMono.txt`) | 500 | ASCII, `·…→←` | 17.7 KB |

Total 140 KB (budget 250 KB). Only Zen Kaku Gothic New 400 is preloaded; all faces use
`font-display: swap`.
