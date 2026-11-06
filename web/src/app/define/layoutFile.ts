// The project's layout file (呼び名 and card places) as the page keeps it. PUT /v1/layout replaces the file whole, so a
// file not read (a newer format, damage, a failed request) is never written over: every change first tries to read it
// again, and once it can be read the editor takes what it holds before anything is written.
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

  /** `adopt`: the file could be read only after the project opened; what it holds goes into the editor. */
  constructor(
    private readonly port: LayoutPort,
    private readonly adopt: (read: Layout) => void,
  ) {}

  /** Reads the file as a project opens; `false` when it could not be. */
  async open(): Promise<boolean> {
    const read = await this.read();
    this.current = read ?? {};
    this.unread = read === null;
    return !this.unread;
  }

  /**
   * Whole names or positions from the editor, asked for only when their turn comes, so they hold what a read before
   * them brought. `false`: not written, the file stays as it is.
   */
  change(part: () => Pick<Layout, 'names'> | Pick<Layout, 'positions'>): Promise<boolean> {
    const next = this.chain.then(() => this.write(part));
    this.chain = next;
    return next;
  }

  private async write(part: () => Pick<Layout, 'names'> | Pick<Layout, 'positions'>): Promise<boolean> {
    if (this.unread) {
      const read = await this.read();
      if (read === null) return false;
      this.unread = false;
      this.current = read;
      this.adopt(read);
    }
    this.current = { ...this.current, format: LAYOUT_FORMAT, ...part() };
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
