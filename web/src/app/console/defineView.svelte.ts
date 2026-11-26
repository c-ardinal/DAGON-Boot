// What 定義 shows of the 構成 being edited: the next step, the problems on the cards, the focal
// number, whether it is saved, and a part dragged from the parts window onto the canvas.
import type { Chip } from '../../graph/Graph.svelte';
import type { Dot } from '../../screens/Focal.svelte';
import type { Part } from '../../screens/define/NodeList.svelte';
import { prepareSentence } from '../band.ts';
import { CANVAS, CLI_LINK, DEFINE, MODE, SAVE, STATUS } from '../copy.ts';
import type { Cli } from '../define/cli.svelte.ts';
import type { Define } from '../define/define.svelte.ts';
import { problemChip } from '../define/problems.ts';
import type { Control } from '../locks.ts';
import type { BuildFlow } from './buildFlow.svelte.ts';
import type { Workspace } from './workspace.svelte.ts';

export interface DefineViewDeps {
  d: Define;
  cli: Cli;
  ws: Workspace;
  build: BuildFlow;
  stopped: (c: Control) => boolean;
  /** Where on the canvas a part dropped at (x, y) would go: under `parent`. */
  dropTarget: (x: number, y: number) => { parent: string | null } | null | undefined;
}

/** A press that moves farther than this is a drag, not a click. */
export const DRAG_PX = 4;

export class DefineView {
  readonly problemCount = $derived(this.deps.d.problems.filter((p) => p.severity === 'error').length);
  /** serve names the nodes that can be built; a node without one has no skeleton yet. */
  readonly needsSkeleton = $derived(!!this.deps.cli.hello?.build_nodes && this.deps.d.config.reach.some((id) => !this.deps.cli.hello?.build_nodes?.includes(id)));
  /** 定義's next step in one sentence (the status row), and its failure cause. */
  readonly next = $derived.by((): { cause?: string; sentence: string } => {
    const { d, cli, ws, build } = this.deps;
    const firstError = d.problems.find((p) => p.severity === 'error');
    if (d.saveFailure) return { cause: SAVE[d.saveFailure].chip, sentence: SAVE[d.saveFailure].sentence };
    if (ws.lockShown === 'unsaved') return { cause: SAVE.unsaved, sentence: MODE.lockedUnsaved.sentence };
    if (ws.lockShown) return { sentence: MODE.lockedProblems.sentence };
    if (cli.state !== 'ready' && !ws.local) return { sentence: CLI_LINK.needCli };
    if (cli.defsFailed) return { cause: DEFINE.defsChip, sentence: DEFINE.defsSentence };
    if (d.unreadable) return { cause: DEFINE.unreadableChip, sentence: DEFINE.unreadableSentence };
    if (firstError) return { sentence: DEFINE.fixSentence };
    if (d.skipped) return { sentence: DEFINE.skippedSentence(d.skipped) };
    if (d.doc.topo.nodes.length === 0) return { sentence: CANVAS.empty };
    return { sentence: prepareSentence({ needsSkeleton: this.needsSkeleton, building: build.state === 'running', built: build.builtRevision === d.revision, progress: build.progress }) };
  });
  readonly savedLabel = $derived(this.deps.ws.local ? '' : this.deps.d.unsaved ? SAVE.unsaved : this.deps.ws.autosave.pending ? STATUS.saving : STATUS.saved);
  /** The problems on their nodes' cards. */
  readonly chips: Record<string, Chip> = $derived(
    Object.fromEntries(
      this.deps.d.problems
        .filter((p) => p.select?.startsWith('node:'))
        .map((p) => [p.select!.slice(5), { chip: `${problemChip(p).mark} ${p.short}`, spoken: p.short, silent: problemChip(p).kind === 'error' }]),
    ),
  );
  /** 定義's focal number: 問題 n, red only above 0; ノード, リンク and ◇ under it. */
  readonly dot: Dot = $derived.by(() => {
    const { d } = this.deps;
    const nodes = d.doc.topo.nodes.length;
    const links = d.doc.topo.links?.length ?? 0;
    return {
      label: DEFINE.dotProblems,
      value: this.problemCount,
      alarm: true,
      sub: [DEFINE.dotNodes, nodes] as const,
      more: [[DEFINE.dotLinks, String(links)], ...(d.skipped ? [[DEFINE.dotSkipped, String(d.skipped)] as const] : [])],
      name: DEFINE.dotProblemsName(this.problemCount, nodes, links),
    };
  });

  /** A part dragged from the parts window to the canvas: a ghost follows the pointer. */
  ghost = $state<{ part: Part; x: number; y: number; shown: boolean } | null>(null);
  /** The node a dragged part would go under (highlighted on the canvas). */
  dropOn = $state<string | null>(null);

  constructor(private readonly deps: DefineViewDeps) {}

  startPart(part: Part, e: PointerEvent): void {
    const x0 = e.clientX;
    const y0 = e.clientY;
    this.ghost = { part, x: x0, y: y0, shown: false };
    const move = (ev: PointerEvent) => {
      if (!this.ghost) return;
      const shown = this.ghost.shown || Math.hypot(ev.clientX - x0, ev.clientY - y0) > DRAG_PX;
      this.ghost = { ...this.ghost, x: ev.clientX, y: ev.clientY, shown };
      this.dropOn = shown ? (this.deps.dropTarget(ev.clientX, ev.clientY)?.parent ?? null) : null;
    };
    const up = (ev: PointerEvent) => {
      removeEventListener('pointermove', move);
      removeEventListener('pointerup', up);
      const g = this.ghost;
      this.ghost = null;
      this.dropOn = null;
      if (!g?.shown) return;
      const t = this.deps.dropTarget(ev.clientX, ev.clientY);
      if (!t || this.deps.stopped('edit')) return;
      this.deps.d.placeAt(g.part.key, g.part.name, t.parent);
    };
    addEventListener('pointermove', move);
    addEventListener('pointerup', up);
  }
}
