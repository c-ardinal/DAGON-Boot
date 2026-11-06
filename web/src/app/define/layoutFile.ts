// The project's layout file (呼び名 and card places) as the page keeps it. PUT /v1/layout replaces the file whole, so a
// file not read (a newer format, damage, a failed request) is never written over: every change first tries to read it
// again, and what was in it stays under the change once it can be read.
import { LAYOUT_FORMAT, type Layout } from './cli.svelte.ts';

export interface LayoutPort {
  layoutRefused: string | null;
  loadLayout(): Promise<Layout | null>;
  saveLayout(layout: Layout): Promise<boolean>;
}

export class LayoutFile {
  /** The file as last read or written. */
  current: Layout = {};
  /** The file was not read: nothing is written. */
  unread = false;
  /** Writes run one after another (PUT /v1/layout is not ordered by itself). */
  private chain: Promise<unknown> = Promise.resolve();

  constructor(private readonly port: LayoutPort) {}

  /** Reads the file as a project opens; `false` when it could not be. */
  async open(): Promise<boolean> {
    const read = await this.read();
    this.current = read ?? {};
    this.unread = read === null;
    return !this.unread;
  }

  /** Whole names or positions from the page. `false`: not written, the file stays as it is. */
  change(part: Pick<Layout, 'names'> | Pick<Layout, 'positions'>): Promise<boolean> {
    const next = this.chain.then(() => this.write(part));
    this.chain = next;
    return next;
  }

  private async write(part: Pick<Layout, 'names'> | Pick<Layout, 'positions'>): Promise<boolean> {
    let merged: Layout = part;
    if (this.unread) {
      const read = await this.read();
      if (read === null) return false;
      this.unread = false;
      this.current = read;
      merged = {
        ...('names' in part ? { names: { ...read.names, ...part.names } } : {}),
        ...('positions' in part ? { positions: { ...read.positions, ...part.positions } } : {}),
      };
    }
    this.current = { ...this.current, format: LAYOUT_FORMAT, ...merged };
    return this.port.saveLayout(this.current);
  }

  private async read(): Promise<Layout | null> {
    this.port.layoutRefused = null;
    try {
      const read = await this.port.loadLayout();
      if (this.port.layoutRefused !== null) return null;
      return read ?? {};
    } catch (e) {
      this.port.layoutRefused = e instanceof Error ? e.message : String(e);
      return null;
    }
  }
}
