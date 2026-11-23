// The node connected to the PC as a device: the Operate session over it, the port the page
// opened (Web Serial, or a development stand-in), and the devices the browser reports
// arriving and leaving.
import type { DevHub } from '../../serial/devHub.ts';
import { MockTransport } from '../../serial/mock.ts';
import { WebSerialTransport, type Transport } from '../../serial/transport.ts';
import { trialMs } from '../../wasm/dagon.ts';
import { webSerial } from '../connect.ts';
import type { Cli } from '../define/cli.svelte.ts';
import { Operate } from '../operate.svelte.ts';

export interface HubPortDeps {
  serial: Serial | undefined;
  /** The development stand-in or the demo's simulator; null on a served page. */
  stand: DevHub | null;
  cli: Cli;
  /** A node's MCU in the 構成 (its definition's [watchdog] trial_ms counts the node's %). */
  mcuOf: (id: string) => string;
}

/** Chrome's SerialConnectionEvent names the port; the spec's event targets it. */
const portOf = (e: Event) => ((e as Event & { port?: SerialPort }).port ?? e.target) as SerialPort;

export class HubPort {
  readonly op: Operate;
  /** The device the page opened as the node connected to the PC. */
  private opened: Transport | null = null;
  private connecting: Promise<void> = Promise.resolve();

  constructor(deps: HubPortDeps) {
    const { stand, cli } = deps;
    const dev = stand !== null;
    const ports = webSerial(deps.serial);
    this.op = new Operate({
      open: async () => {
        const t = stand?.transport ?? null;
        if (t) await t.open();
        this.opened = t ?? (await ports.chooseAndOpen());
        return this.opened;
      },
      // The stand-in PC-connected node comes back by itself; a real one is chosen again in the browser.
      reselect: async () => {
        const opened = this.opened;
        if (dev && opened) {
          if (opened instanceof MockTransport) opened.replug();
          await opened.open();
          return opened;
        }
        // `opened` follows only once Operate took it as this PC-connected node (reconnectChosen).
        return ports.chooseAndOpen();
      },
      timer: true,
      // The simulator's clock runs at the demo's speed; a real PC-connected node keeps the default.
      get clock() {
        return stand?.clock;
      },
      // The CLI gave the node connected to the PC back: the device the page had, opened again without the chooser.
      again: async () => {
        if (!this.opened) throw new Error('no device');
        await this.opened.open();
        return this.opened;
      },
      // The project's files through dagon serve only (stand-ins in development); nothing public.
      latest: async () => (cli.state === 'ready' ? await cli.latestRelease() : null) ?? (await stand?.latest?.()) ?? null,
      bootcore: async (id) => (cli.state === 'ready' ? await cli.bootcore(id) : null) ?? (await stand?.bootcore?.(id)) ?? null,
      stage0: async (id) => (cli.state === 'ready' ? await cli.stage0(id) : null),
      // Every package written through DAGON is kept in the project (serve); without serve there is nowhere to keep it.
      keep: async (bytes) => (cli.state === 'ready' ? (await cli.keep(bytes)) !== null : true),
      trialMs: (id) => trialMs(deps.mcuOf(id)),
    });
    cli.onDevice = (need) => (need ? this.op.handOver() : this.op.cliReturned());
    ports.on('disconnect', (e) => {
      if (this.opened instanceof WebSerialTransport && portOf(e) === this.opened.port) this.op.lose();
    });
    // An allowed device came (back): while dagon-host reopens the rebooted PC-connected node (hubRestarting) it is a
    // candidate; once that is over (reconnectNeeded) the page takes it by itself when it is the node connected to the
    // PC on trial with the version just sent.
    ports.on('connect', (e) => {
      const port = portOf(e);
      // One device at a time: candidates reported together are judged in turn.
      this.connecting = this.connecting.then(() => this.arrived(port)).catch(() => undefined);
    });
  }

  /** "再接続": the device the user chose, kept only when Operate took it as this PC-connected node. */
  async reconnectChosen(): Promise<void> {
    if (await this.op.reconnectHub()) this.opened = this.op.device;
  }

  private async arrived(port: SerialPort): Promise<void> {
    if (this.opened instanceof WebSerialTransport) this.opened.arrived(port);
    if (this.op.state !== 'reconnectNeeded') return;
    const t = new WebSerialTransport(port);
    const took = await this.op.reconnectHub(async () => {
      await t.open();
      return t;
    }, true);
    if (took) this.opened = t;
  }
}
