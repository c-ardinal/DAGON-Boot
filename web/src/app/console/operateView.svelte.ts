// What 運用 shows of the Operate session: the band, the focal number, the cards' chips and
// water, the timeline, the notices a write raises and the modals that need the user.
import { untrack } from 'svelte';
import type { Chip } from '../../graph/Graph.svelte';
import type { Dot } from '../../screens/Focal.svelte';
import type { StageEvent } from '../../screens/Timeline.svelte';
import { bandOf, connectNotice } from '../band.ts';
import { BAND, CANVAS, CONN, DIFF, HUB_TOGGLE, PRODUCT, RECONNECT, SIM, STEP, TIME, WRITE, fill as fillIn } from '../copy.ts';
import type { Define } from '../define/define.svelte.ts';
import { topoDiff } from '../define/diff.ts';
import type { Topo } from '../define/model.ts';
import { HUB_LOCKED, hubOf, view } from '../machine.ts';
import { tokenMs } from '../motion.ts';
import type { Operate } from '../operate.svelte.ts';
import { waterOf, type ReleaseFile } from '../writeProgress.ts';
import { hubIn } from '../sample.ts';
import { logStages } from '../stageLog.svelte.ts';
import type { Feedback } from './feedback.svelte.ts';
import { cardChipOf, CONTROL_OF, PERCENT, RUNS, UPDATING, WRITABLE } from './states.ts';
import type { WriteFlow } from './writeFlow.svelte.ts';

export interface OperateViewDeps {
  op: Operate;
  d: Define;
  writes: WriteFlow;
  feedback: Feedback;
  simulated: boolean;
  demoSite: boolean;
  /** The status row's cause and sentence in the mode shown. */
  next: () => { cause?: string; sentence: string };
}

/** The automatic reconnect's countdown steps (hubRestarting). */
const RECONNECT_STEP_S = 5;

export class OperateView {
  /** Clock for the %, the reconnect countdown (dagon-host's clock) and the elapsed time (wall clock), read while they show. */
  nowMs = $state(0);
  wallMs = $state(Date.now());
  readonly hub = $derived(hubOf(this.deps.op.state));
  readonly connected = $derived(!['idle', 'lost'].includes(this.hub));
  /** The node connected to the PC this page talks to (the one writing reaches), if the 構成 has one. */
  readonly hubNode = $derived(hubIn(this.deps.op.config, true));
  readonly hubName = $derived(this.hubNode ? `[${this.hubNode.no}] ${this.hubNode.name}` : '');
  /** The node a direct write over USB starts with: the one connected to the PC (it reaches the rest). */
  readonly hubId = $derived((this.hubNode ?? hubIn(this.deps.op.config, false))?.id ?? null);
  /** Why the header's 接続 / 切断 cannot be used now. */
  readonly hubLocked = $derived(HUB_LOCKED.has(this.deps.op.state) ? (HUB_TOGGLE.locked[this.deps.op.state] ?? null) : null);
  /** The locked toggle was pressed: its reason stands in the band until the state changes. */
  hubNotice = $state<{ at: string; why: string } | null>(null);
  readonly step = $derived(view(this.deps.op.state, this.deps.op.ctx).step);
  readonly updating = $derived(UPDATING.has(this.deps.op.state));
  /** Seconds left of the automatic reconnect, in 5 s steps (hubRestarting). */
  readonly reconnectLeft = $derived.by(() => {
    const { op } = this.deps;
    if (op.reconnectUntil === null || op.state !== 'hubRestarting') return null;
    return Math.ceil(Math.max(0, (op.reconnectUntil - this.nowMs) / 1000) / RECONNECT_STEP_S) * RECONNECT_STEP_S;
  });

  /** The reconnect dialog only after the automatic reconnect failed (reconnectNeeded); Escape leaves the same button in the band. */
  dialogDismissed = $state(false);
  readonly dialogOpen = $derived(this.deps.op.state === 'reconnectNeeded' && !this.dialogDismissed);
  readonly band = $derived.by(() => {
    const { op, writes } = this.deps;
    const b = bandOf(op.state, op.ctx, {
      stuck: op.recoverTarget?.name,
      responding: op.responding,
      unwritten: Object.values(op.progress).filter((p) => p.stage === 'unwritten').length,
      stopped: op.failed ? op.progress[op.failed]?.stage : undefined,
      rest: op.rest && op.state === 'fileReady',
      finish: op.finished,
      refused: op.returnRefused?.kind,
      sim: this.deps.simulated,
      built: writes.latest !== null || writes.kept.length > 0,
    });
    if (this.dialogOpen) return { ...b, primary: null };
    // The demo page reads no files: its prepared package is what 書込 writes.
    if (this.deps.demoSite && op.state === 'ready') return { ...b, sentence: SIM.siteWrite };
    if (op.latestMissing) return { ...b, ...BAND.noRelease };
    const notice = this.hubNotice && this.hubNotice.at === op.state ? this.hubNotice.why : null;
    if (notice) return { ...b, chip: undefined, pair: undefined, sentence: notice };
    return b;
  });

  readonly dot = $derived.by((): Dot | null => {
    const { op } = this.deps;
    if (!op.config || !this.connected || op.state === 'checking') return null;
    if (PERCENT.has(op.state)) {
      const halted = op.state === 'stopped';
      const overall = op.overallAt(this.nowMs);
      const progress = { label: CANVAS.dotProgress, value: overall, unit: halted ? CANVAS.dotStopped : CANVAS.dotPercent, name: CANVAS.dotProgressName(overall, halted) };
      if (op.startedAt === null) return progress;
      // 経過 under the number: the one time that is measured, not guessed.
      const gone = Math.max(0, this.wallMs - op.startedAt);
      return { ...progress, sub: [TIME.elapsed, TIME.clock(gone)] as const };
    }
    if (op.state === 'scanning') return { label: CANVAS.dotFound, value: op.found, name: CANVAS.dotFoundName(op.found) };
    // Out of the nodes updating reaches: the others are never watched.
    const n = op.reach.length;
    return { label: CANVAS.dotResponding, value: op.responding, of: n, name: CANVAS.dotRespondingName(op.responding, n) };
  });

  /** The 構成 differences of a package built for another 構成. */
  readonly fileDiff = $derived(this.deps.op.state === 'otherTopology' && this.deps.op.fileTopology ? topoDiff(this.deps.d.doc.topo, this.deps.op.fileTopology as unknown as Topo, this.deps.d.doc.names) : []);
  /** On the cards: while writing, each node's stage; otherwise the nodes a refused package differs at. */
  readonly chips: Record<string, Chip> = $derived.by(() => {
    const { op } = this.deps;
    if (!this.updating) return Object.fromEntries(this.fileDiff.flatMap((r) => r.nodes).map((id) => [id, { chip: DIFF.chip, spoken: DIFF.spoken, silent: true }]));
    return Object.fromEntries(
      Object.entries(op.progress).flatMap(([id, p]): [string, Chip][] => {
        const c = cardChipOf(p, op.fileVersions[id]);
        if (!c) return [];
        // The countdown beside the rebooting PC-connected node while the page reconnects by itself.
        const left = this.reconnectLeft;
        return [[id, p.stage === 'rebooting' && left !== null ? { ...c, note: `${RECONNECT.auto} ${RECONNECT.left(left)}` } : c]];
      }),
    );
  });
  /** The water in each node while it is written (writing, hubRestarting, recovering). */
  readonly fill = $derived(PERCENT.has(this.deps.op.state) ? Object.fromEntries(Object.keys(waterOf(this.deps.op.progress)).map((id) => [id, this.deps.op.percentOf(id, this.nowMs)])) : {});

  /** The refusal modal: why the package cannot be written and what to do; closing leaves the band's buttons. */
  refusalClosed = $state<ReleaseFile | null>(null);
  readonly refusalOpen = $derived(this.deps.op.file !== null && (this.deps.op.state === 'fileRefused' || this.deps.op.state === 'otherTopology') && this.refusalClosed !== this.deps.op.file);
  /** A connection the user must fix: its next step, until closed (or the state moves on). */
  connectClosed = $state(false);
  readonly connectModal = $derived(connectNotice(this.deps.op.state) === 'modal' && !this.connectClosed);
  /** A write that stopped at a node (back on its previous version, or not written): what happened and what to do, until closed. */
  stopClosed = $state<string | null>(null);
  readonly stopNode = $derived(this.deps.op.state === 'stopped' && this.deps.op.failed ? (this.deps.op.config?.nodes.find((n) => n.id === this.deps.op.failed) ?? null) : null);
  readonly stopOpen = $derived(this.stopNode !== null && this.stopClosed !== this.deps.op.failed);
  /** What the node or the link said, in the user's words: the cause, then what to do (the band's sentence when nothing more is known). */
  readonly failWhy = $derived.by(() => {
    const { op } = this.deps;
    const p = op.failed ? op.progress[op.failed] : undefined;
    const w = (p?.reason && WRITE.why[p.reason]) || (p?.stage && WRITE.why[p.stage]);
    const next = this.deps.next();
    return w ?? { reason: next.cause ?? WRITE.failed, fix: next.sentence };
  });

  /** 書込: the node connected to the PC answered nothing (silentHub) → writing directly over USB; otherwise the next step of writing through DAGON. */
  readonly writeAction = $derived.by(() => {
    const { op } = this.deps;
    if (op.state === 'silentHub') return null;
    if (op.state === 'ready' && !op.latestMissing && this.band.secondary.some((b) => b.action === 'useLatest')) return 'useLatest';
    return this.band.primary?.action ?? null;
  });
  readonly writeControl = $derived(WRITABLE.has(this.deps.op.state) ? 'update' : this.writeAction ? CONTROL_OF[this.writeAction] : 'flash');
  readonly writeOff = $derived(this.deps.op.state === 'choosing' || (this.deps.op.state !== 'silentHub' && !WRITABLE.has(this.deps.op.state) && (this.writeAction === null || !RUNS.has(this.writeAction))));

  /** The timeline: each node's stage changes since the update began; it opens with the update. */
  tlEvents = $state<StageEvent[]>([]);
  private tlStart = 0;
  readonly tlNow = $derived(this.tlStart && PERCENT.has(this.deps.op.state) ? Math.max(0, this.wallMs - this.tlStart) : Math.max(0, ...this.tlEvents.map((e) => e.t)));
  private wasConnected = false;

  /** Call it where an effect may be made (a component's script). */
  constructor(private readonly deps: OperateViewDeps) {
    const { op, feedback } = deps;
    $effect(() => {
      if (!PERCENT.has(op.state)) return;
      const tick = () => {
        this.nowMs = op.now();
        this.wallMs = Date.now();
      };
      tick();
      const t = setInterval(tick, tokenMs('--dur-tick'));
      return () => clearInterval(t);
    });
    logStages(op, {
      begin: (start) => {
        this.tlStart = start;
        this.tlEvents = [];
      },
      add: (events) => (this.tlEvents = [...this.tlEvents, ...events]),
    });
    $effect(() => {
      void op.state;
      this.connectClosed = false;
    });
    $effect(() => {
      if (op.state !== 'stopped') this.stopClosed = null;
    });
    $effect(() => {
      if (op.state !== 'reconnectNeeded') this.dialogDismissed = false;
    });
    $effect(() => {
      const s = this.step;
      const s3 = s === null ? null : STEP.of3(s);
      document.title = s3 === null || s3 === 'recover' ? PRODUCT : `${String(s3).padStart(2, '0')} ${STEP.of} ${STEP.names[s3]}｜${PRODUCT}`;
    });
    $effect(() => {
      const c = this.connected;
      untrack(() => {
        if (c && !this.wasConnected && this.hubName) feedback.notice = { word: fillIn(CONN.toast.connected, { node: this.hubName }) };
        if (!c && this.wasConnected && this.hub === 'lost') feedback.notice = { word: fillIn(CONN.toast.lost, { node: this.hubName }), error: true };
        this.wasConnected = c;
      });
    });
    $effect(() => {
      if (op.state !== 'done') return;
      untrack(() => {
        const f = op.finished;
        const wrote = f?.kind === 'updated' ? (f.wrote ?? op.targets.length) : op.targets.length;
        feedback.notice = { word: wrote ? fillIn(WRITE.toast.done, { n: wrote }) : WRITE.toast.none };
      });
    });
    // A failure or warning the user must notice: its short word on a toast, the sentence behind it.
    $effect(() => {
      // 準備 has its own sentence, but a connection that failed there is told all the same.
      const bad = connectNotice(op.state);
      const next = deps.next();
      const cause = bad === 'toast' ? this.band.chip : next.cause;
      const detail = bad === 'toast' ? this.band.sentence : next.sentence;
      // What needs the user opens its modal instead.
      if (cause && bad !== 'modal' && !untrack(() => this.refusalOpen || this.stopNode !== null)) untrack(() => (feedback.notice = { word: cause, error: cause.startsWith('×'), detail }));
    });
  }

  /** The locked 接続 / 切断 was pressed: its reason stands in the band until the state changes. */
  noteHubLocked(why: string): void {
    this.hubNotice = { at: this.deps.op.state, why };
  }
}
