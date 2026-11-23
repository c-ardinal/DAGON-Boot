// 書込 beyond the band's steps: the version picker, a write while watching, a write to one
// node (through DAGON, the MCU's ROM loader or serve's direct write over USB) and the package
// file the user chooses.
import { untrack } from 'svelte';
import type { Notice } from '../../screens/Toast.svelte';
import { releaseVersions, versionText } from '../../wasm/dagon.ts';
import { BAND, BUILD, WRITE } from '../copy.ts';
import type { Cli, Job } from '../define/cli.svelte.ts';
import { answerConsent } from '../factory.ts';
import { keptChoices, type Kept } from '../kept.ts';
import type { Control } from '../locks.ts';
import type { Operate } from '../operate.svelte.ts';

export interface WriteDeps {
  op: Operate;
  cli: Cli;
  /** The stand-in's release without serve (development, the demo page). */
  standLatest?: () => Promise<Uint8Array | null>;
  stopped: (c: Control) => boolean;
  tell: (n: Notice) => void;
  /** Opens the hidden file chooser. */
  chooseFile: () => void;
}

/** A package by its name and bytes. */
export interface Chosen {
  name: string;
  bytes: Uint8Array;
}

export class WriteFlow {
  /** 書込's version picker: the latest build (the project's newest release) first, then the kept packages; writing an older one is going back. */
  latest = $state<{ bytes: Uint8Array; label: string } | null>(null);
  kept = $state<Kept[]>([]);
  readonly versionChoices = $derived([...(this.latest ? [{ id: 'latest', label: this.latest.label }] : []), ...keptChoices(this.kept).filter((c) => c.label !== this.latest?.label)]);
  /** The silent node whose ROM write waits for the user's word. */
  romAsk = $state<string | null>(null);
  readonly romNode = $derived(this.deps.op.config?.nodes.find((n) => n.id === this.romAsk) ?? null);
  /** Writing directly over USB (serve's flash): the node, and serve's job once it has taken it. */
  factory = $state<{ node: string; id: number | null } | null>(null);
  readonly factoryNode = $derived(this.deps.op.config?.nodes.find((n) => n.id === this.factory?.node) ?? null);
  readonly factoryJob: Job | null = $derived(this.factory?.id == null ? null : (this.deps.cli.jobs.find((j) => j.id === this.factory?.id) ?? null));

  /** The file chooser is open for a one-node write that found no release. */
  private pickForRecovery: { id?: string } | null = null;
  // Pressing 書込 in the modal is the consent serve's write asks for.
  private readonly consentAnswered = new Set<string>();

  /** Call it where an effect may be made (a component's script). */
  constructor(private readonly deps: WriteDeps) {
    $effect(() => {
      const j = this.factoryJob;
      void j?.state;
      untrack(() => answerConsent(j, (job, text) => deps.cli.answer(job, text), this.consentAnswered));
    });
    $effect(() => {
      const s = this.factoryJob?.state;
      if (s === 'succeeded') untrack(() => deps.tell({ word: WRITE.state.done }));
      if (s === 'cancelled') untrack(() => (this.factory = null));
    });
    $effect(() => {
      if (deps.op.keepFailed) untrack(() => deps.tell({ word: WRITE.keepFailed.label, error: true, detail: WRITE.keepFailed.detail }));
    });
  }

  /** One version for all of a package's nodes names it; otherwise it is 最新のビルド. */
  private labelOf(bytes: Uint8Array): string {
    const one = [...new Set([...releaseVersions(bytes).values()].map(versionText))];
    return one.length === 1 ? one[0] : WRITE.latest;
  }

  async loadVersions(): Promise<void> {
    const { cli, standLatest } = this.deps;
    const served = cli.state === 'ready';
    // Without serve the stand-in's release is the latest build; nothing is kept.
    const fetchLatest = served ? () => cli.latestRelease() : standLatest;
    if (!fetchLatest) return;
    const bytes = await fetchLatest().catch(() => null);
    this.latest = bytes ? { bytes, label: this.labelOf(bytes) } : null;
    this.kept = served ? await cli.keptList().catch(() => []) : [];
  }

  /** 書込 while watching: the version (the latest build unless one was chosen), checked against the 構成, to the checked cards. */
  async writeVersion(id = this.latest ? 'latest' : (this.kept[0]?.id ?? 'latest')): Promise<void> {
    const { op, cli, tell } = this.deps;
    if (this.deps.stopped('update')) return;
    const latest = this.latest;
    const pkg: Chosen | null = id === 'latest' ? (latest ? { name: `${latest.label}.dgb`, bytes: latest.bytes } : null) : await cli.keptBytes(id).then((b) => (b ? { name: `${id}.dgb`, bytes: b } : null));
    if (!pkg) return tell({ word: BAND.noRelease.chip, error: true, detail: BAND.noRelease.sentence });
    const no = await op.noWrite(pkg.bytes);
    if (no === 'noneChecked') return tell({ word: WRITE.noneChecked.label, detail: WRITE.noneChecked.detail });
    if (no) {
      const said = WRITE.alreadyOn(versionText(no.same), no.coreToo);
      return tell({ word: said.label, detail: said.detail });
    }
    op.takeFile(pkg.name, pkg.bytes);
    if (op.state === 'fileReady') await op.update();
  }

  /** 「書込」 on one node: the way it answers decides what comes next. */
  async writeOne(id?: string, chosen?: Chosen, again = false): Promise<void> {
    const { op } = this.deps;
    if (this.deps.stopped('recover')) return;
    const r = await (again ? op.retry() : op.writeNode({ id, chosen }));
    const node = id ?? op.recoverTarget?.id ?? null;
    if (r === 'needFile') {
      this.pickForRecovery = { id };
      this.deps.chooseFile();
    } else if (r === 'direct' && node) this.factory = { node, id: null };
    else if (r === 'confirm') this.romAsk = node;
  }

  /** The user's word for the ROM write. */
  writeRom(): void {
    const id = this.romAsk;
    this.romAsk = null;
    if (id && !this.deps.stopped('recover')) void this.deps.op.writeNode({ id, confirmed: true });
  }

  /** The file chooser closed: the package goes to the one-node write that asked for it, or to the band's write. */
  async picked(f: File | undefined): Promise<void> {
    if (f && this.deps.stopped(this.pickForRecovery ? 'recover' : 'file')) return void (this.pickForRecovery = null);
    const forRecovery = this.pickForRecovery;
    this.pickForRecovery = null;
    if (!f) return;
    const bytes = new Uint8Array(await f.arrayBuffer());
    if (forRecovery) void this.writeOne(forRecovery.id, { name: f.name, bytes });
    else this.deps.op.takeFile(f.name, bytes);
  }

  /** The chooser was closed without a file. */
  pickCancelled(): void {
    this.pickForRecovery = null;
  }

  /** The direct write's 書込: serve's flash job for the node. */
  async factoryWrite(): Promise<void> {
    const n = this.factoryNode;
    if (!n || this.deps.stopped('flash')) return;
    const j = await this.deps.cli.flash(n.id).catch(() => {
      this.deps.tell({ word: WRITE.failed, error: true, detail: BUILD.startFailed });
      return null;
    });
    this.factory = { node: n.id, id: j?.id ?? null };
  }
}
