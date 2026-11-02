// Prepares the public site in dist/ after `npm run build` (it does not
// publish anything): the page plus the four files it serves next to itself.
//
//   release/latest.dgb          release for "配布中の最新バージョンを使う" and recovery
//   release/seg-bootcore.img    signed F401 boot core for the ROM path
//   sample/desk-demo.yaml       the sample configuration
//   sample/desk-demo.bin        its canonical topology binary
//
// Each file is checked with the dagon-wasm the page was built with (its
// release key): the release must verify and be built for desk-demo, the boot
// core must verify. Prints the SHA-256 of every file for the owner's go-ahead.
//
// npm run site -- [--release <dgb>] [--bootcore <img>]
// Defaults: ../out/release-latest.dgb and ../out/provision/seg/bootcore.img
// (dagon.toml's release and `dagon provision seg`).
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { initSync, check_bootcore_for, check_release_for, topo_compile } from '../src/wasm/pkg/dagon_wasm.js';

const web = new URL('..', import.meta.url);
const repo = new URL('..', web);
const dist = new URL('dist/', web);
const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? pathToFileURL(resolve(process.argv[i + 1])) : new URL(fallback, repo);
};

initSync({ module: await readFile(new URL('src/wasm/pkg/dagon_wasm_bg.wasm', web)) });

const yaml = await readFile(new URL('topology/desk-demo.yaml', repo), 'utf8');
const topology = topo_compile(yaml);
const release = await readFile(arg('release', 'out/release-latest.dgb'));
const bootcore = await readFile(arg('bootcore', 'out/provision/seg/bootcore.img'));
check_release_for(release, topology);
check_bootcore_for(bootcore);

await readFile(new URL('index.html', dist)).catch(() => {
  throw new Error('dist/index.html is missing: run `npm run build` first');
});
await mkdir(new URL('release/', dist), { recursive: true });
await mkdir(new URL('sample/', dist), { recursive: true });
const files = {
  'release/latest.dgb': release,
  'release/seg-bootcore.img': bootcore,
  'sample/desk-demo.yaml': Buffer.from(yaml),
  'sample/desk-demo.bin': topology,
};
for (const [path, bytes] of Object.entries(files)) await writeFile(new URL(path, dist), bytes);

const sha = (b) => createHash('sha256').update(b).digest('hex');
console.log('dist/ prepared (not published):');
console.log(`  page              index.html, assets/ (wasm ${sha(await readFile(new URL('src/wasm/pkg/dagon_wasm_bg.wasm', web))).slice(0, 16)}…)`);
for (const [path, bytes] of Object.entries(files)) console.log(`  ${path.padEnd(26)} ${String(bytes.length).padStart(7)} B  sha256 ${sha(bytes)}`);
