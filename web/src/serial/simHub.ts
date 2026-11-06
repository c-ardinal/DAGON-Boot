// The simulator (dagon-sim's SimHub through dagon-wasm's Simulator) as a
// Transport and the clock dagon-host runs on: the same HostLink path as Web
// Serial, with the F401's ROM loader simulated. Answers arrive when the links
// and flash would have carried them (the model `dagon demo` uses too). Present
// only in builds with the `sim` feature (tests, `npm run dev`, the demo page);
// `simulator()` is null otherwise.
import * as pkg from '../wasm/pkg/dagon_wasm.js';
import type { LinkClock } from './hostLink.ts';
import type { Transport } from './transport.ts';

interface SimHub {
  write(bytes: Uint8Array): void;
  read(): Uint8Array;
  /** When the next answer arrives on the simulator's clock; -1 when none is on its way. */
  nextDue(): number;
  now(): number;
  advance(ms: number): void;
  /** Erases the I2C node at `addr` (only its ROM loader is left). */
  wipe(addr: number): boolean;
  /** The watch's next version crashes at boot, so its write goes back (`dagon demo --sim-fail watch`). */
  failWatch(): boolean;
}

type SimulatorClass = new (seed: number) => SimHub;

export interface SimLink {
  transport: Transport;
  clock: LinkClock;
  /** The simulated hub, made on first use (dagon-wasm must be loaded by then). */
  hub: () => SimHub;
}

/**
 * A simulator, or null in builds without it. `prepare` runs once on the new hub. With `pace`
 * (simulated ms per wall ms) waiting takes wall time too, so a write runs about as long as on
 * hardware; without it the simulator's clock jumps ahead at once (tests).
 */
export function simulator(seed = 1, prepare?: (d: SimHub) => void, pace?: number): SimLink | null {
  const Make = (pkg as unknown as Record<string, SimulatorClass | undefined>).Simulator;
  if (!Make) return null;
  let made: SimHub | null = null;
  const hub = (): SimHub => {
    if (!made) {
      made = new Make(seed);
      prepare?.(made);
    }
    return made;
  };
  /** The simulator's clock moves on by `ms`; with `pace` the wall clock waits as long. */
  const pass = (ms: number): Promise<void> => {
    hub().advance(ms);
    return pace ? new Promise((r) => setTimeout(r, ms / pace)) : Promise.resolve();
  };
  let open = false;
  const transport: Transport = {
    open: async () => void (open = true),
    close: async () => void (open = false),
    async write(frame) {
      if (!open) throw new Error('port not open');
      hub().write(frame);
    },
    // Waits up to `timeoutMs` on the simulator's clock for the next answer to arrive.
    async read(timeoutMs) {
      const h = hub();
      let b = h.read();
      if (!b.length) {
        const due = h.nextDue();
        await pass(Math.max(0, Math.min(due < 0 ? timeoutMs : due - h.now(), timeoutMs)));
        b = h.read();
      }
      return b.length ? b : null;
    },
  };
  const clock: LinkClock = {
    now: () => hub().now(),
    sleep: (ms) => (pace ? pass(ms) : (hub().advance(ms), new Promise((r) => setTimeout(r, 0)))),
  };
  return { transport, clock, hub };
}
