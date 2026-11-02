// The byte link dagon-host runs on (dagon-wasm's HostLink), over a frame
// Transport: received frames are handed out as bytes, in pieces when the
// procedure asks for less than a whole frame.
import type { HostLink } from '../wasm/pkg/dagon_wasm.js';
import type { Transport } from './transport.ts';

/** How long one read waits for a frame before reporting "nothing yet". */
const READ_WAIT_MS = 5;

export function hostLink(t: Transport): HostLink {
  let rest: Uint8Array = new Uint8Array(0);
  return {
    write: (bytes) => t.write(bytes.slice()),
    async read(max) {
      if (rest.length === 0) rest = (await t.read(READ_WAIT_MS)) ?? new Uint8Array(0);
      const out = rest.subarray(0, max);
      rest = rest.subarray(out.length);
      return out.slice();
    },
    async reopen() {
      rest = new Uint8Array(0);
      await t.close();
      await t.open();
    },
    now: () => performance.now(),
    sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
  };
}
