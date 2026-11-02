// Builds crates/dagon-wasm into src/wasm/pkg with wasm-pack. The release key
// comes from DAGON_PUBKEY (the .pub file from `dagon key gen`, or its hex);
// `--test-key` opts in to the public test key when DAGON_PUBKEY is unset
// (the stand-in hubs and the tests), as DAGON_ALLOW_TEST_KEY=1 does; `--sim`
// adds the simulated desk (SimDesk, dagon-sim) for the tests and `?sim`.
import { spawnSync } from 'node:child_process';

const testKey = process.argv.includes('--test-key');
const env = testKey ? { ...process.env, DAGON_ALLOW_TEST_KEY: '1' } : process.env;
const args = ['build', '../crates/dagon-wasm', '--release', '--target', 'web', '--no-pack', '--out-dir', '../../web/src/wasm/pkg'];
if (process.argv.includes('--sim')) args.push('--features', 'sim');
const r = spawnSync('wasm-pack', args, { stdio: 'inherit', env, shell: process.platform === 'win32' });
process.exit(r.status ?? 1);
