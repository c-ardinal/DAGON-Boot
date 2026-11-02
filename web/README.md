# web

Browser UI for DAGON Boot: Svelte 5 + Vite + TypeScript, built for Chrome / Edge 89 and later
(the first versions with Web Serial). Screen text is Japanese only and lives in `src/app/copy.ts`.

## Commands

Needs the `wasm32-unknown-unknown` Rust target (listed in `rust-toolchain.toml`) and `wasm-pack`
0.15.1 on `PATH`; `wasm-pack` fetches the matching `wasm-bindgen` and `wasm-opt` itself.

```sh
npm install
npm run wasm      # build crates/dagon-wasm into src/wasm/pkg (also run before dev, build and test)
npm run dev       # http://127.0.0.1:5173
npm run build     # svelte-check, then vite build into dist/ (target chrome89)
npm run preview   # serves dist/ on http://127.0.0.1:4173
npm test          # Vitest
npm run fonts     # re-fetch the font subsets after screen text changes
```

Query parameters for development (`npm run dev` only; production builds leave the stand-in hub out):

| Parameter | Effect |
|---|---|
| `?mock` | Uses an in-memory desk hub instead of Web Serial: the sample hub, the 7-segment node on I2C 0x42 and the watch on UART, all on 1.0.0. It answers HELLO, STATUS, RELAY to the two nodes and the topology transfer, so finding, sending and monitoring run through `dagon-host` unchanged. |
| `?mock=silent` | In-memory device that never answers (no-response path). |
| `&quiet=uart,i2c:66` | With `?mock`: these nodes do not answer RELAY (not-answering path after two monitoring rounds). |

A simulated hub plugs in as one more `Transport` in `src/serial/devHub.ts`, under its own parameter.

## Layout

| Path | Contents |
|---|---|
| `src/serial/frame.ts` | DAGON link v1 frame codec and stream deframer (same bytes as `crates/dagon-proto`; tests compare against `crates/dagon-format/golden/frame_*.bin`). |
| `src/serial/transport.ts` | `Transport` (open / close / write frame / read frame with timeout) and the Web Serial implementation. |
| `src/serial/hello.ts` | HELLO payload types and the response encoder the mock uses. |
| `src/serial/mock.ts` | In-memory desk hub (see `?mock`) in the same frame format. |
| `src/serial/devHub.ts` | Picks the stand-in hub from the page URL. |
| `src/serial/hostLink.ts` | The byte link `dagon-host` runs on, over a `Transport`. |
| `src/wasm/dagon.ts` | The only entry to `crates/dagon-wasm`: HELLO request / response, version text, board table, topology check and compile, release verification. `src/wasm/pkg/` is build output and not committed. |
| `src/app/machine.ts` | Operating-mode state machine: states, main and side actions, transitions (`machine.test.ts` checks one main action at most and no dead ends). |
| `src/app/operate.svelte.ts` | The operating-mode session: connect, find, send, monitoring every `watch_interval_ms()`, all through `dagon-host` in `dagon-wasm`. |
| `src/app/band.ts` | Bottom band per state: cause chip, sentence, named buttons. |
| `src/app/sample.ts` | The sample desk set-up (`topology/desk-demo.yaml` plus names and wiring). |
| `src/app/connect.ts` | Device chooser and error mapping. |
| `src/app/copy.ts` | All screen and announcement text. |
| `src/graph/` | The layered graph: own SVG (layers, wire bundles, ties, ports) + HTML (nodes, chips), roving focus with arrow keys. |
| `src/screens/` | Header, bottom band, update order (left column), detail (right column). |
| `src/styles/tokens.css` | Design tokens: colour, type, spacing, radius, z-index, motion. |

`Transport` carries whole frames: the page owns choosing and opening the device, and the protocol
steps belong to `dagon-host`, which will receive a `Transport` through `dagon-wasm`.

## Web Serial on http://127.0.0.1

The Web Serial API is exposed only in secure contexts. Loopback origins (`http://127.0.0.1`,
`http://localhost`) count as potentially trustworthy, so `vite preview` / `dagon serve` pages on
127.0.0.1 get `navigator.serial` without HTTPS. Checked in headless Edge 156 against
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
| Zen Kaku Gothic New | github.com/googlefonts/zen-kakugothic | OFL 1.1 (`OFL-ZenKakuGothicNew.txt`) | 400, 500, 700 | ASCII + every non-ASCII character in `src/` | 3 × 38 KB |
| JetBrains Mono | github.com/JetBrains/JetBrainsMono | OFL 1.1 (`OFL-JetBrainsMono.txt`) | 500 | ASCII, `·…→←` | 17.7 KB |

Total 106 KB (budget 250 KB). Only Zen Kaku Gothic New 400 is preloaded; all faces use
`font-display: swap`.
