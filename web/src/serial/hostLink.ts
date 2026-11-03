// The byte link dagon-host runs on (dagon-wasm's HostLink), over a frame
// Transport: received frames are handed out as bytes, in pieces when the
// procedure asks for less than a whole frame.
import type { HostLink } from '../wasm/pkg/dagon_wasm.js';
import { MAX_PAYLOAD_USB } from './frame.ts';
import type { Transport } from './transport.ts';

/** How long one read waits for a frame before reporting "nothing yet". */
const READ_WAIT_MS = 5;

/** The clock dagon-host's timeouts run on. */
export interface LinkClock {
  now(): number;
  sleep(ms: number): Promise<void>;
}

export const REAL_CLOCK: LinkClock = {
  now: () => performance.now(),
  sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
};

/** A HostLink whose device can be replaced (the hub chosen again after its reboot). */
export interface SwapLink extends HostLink {
  use(t: Transport): void;
}

export function hostLink(first: Transport, clock: LinkClock = REAL_CLOCK): SwapLink {
  let t = first;
  let rest: Uint8Array = new Uint8Array(0);
  return {
    write: (bytes) => t.write(bytes.slice()),
    // The link's own size and reboot behaviour (dagon-host asks; USB CDC when the transport does not say).
    maxPayload: () => t.maxPayload?.() ?? MAX_PAYLOAD_USB,
    reconnectKind: () => t.reconnectKind?.() ?? 'reopen',
    async read(max) {
      if (rest.length === 0) rest = (await t.read(READ_WAIT_MS)) ?? new Uint8Array(0);
      const out = rest.subarray(0, max);
      rest = rest.subarray(out.length);
      return out.slice();
    },
    async reopen() {
      rest = new Uint8Array(0);
      // Closing a device that already left fails; opening is what matters.
      await t.close().catch(() => undefined);
      await t.open();
    },
    now: () => clock.now(),
    sleep: (ms) => clock.sleep(ms),
    use(next) {
      rest = new Uint8Array(0);
      t = next;
    },
  };
}
