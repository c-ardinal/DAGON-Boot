// Operating-mode session: drives the state machine with what the user does
// and what dagon-host (through dagon-wasm) reports. The page only chooses and
// opens the device; every request, timeout and retry is dagon-host's.
// The link to the node connected to the PC and the watch are OperateLink's; here are the update,
// the reconnect after the node connected to the PC reboots, and recovery.
import { isNote, type Transport } from '../serial/transport.ts';
import { LOCAL, PHASE, check_release_for, factorySlot, releaseAgentDigest, releaseCoreVersions, releaseVersions, topo_compile, versionText, type Host, type NodeResult, type ConfirmResult, type RecoverResult, type Refusal, type UpdateEvent } from '../wasm/dagon.ts';
import type { RollbackRecord } from './agent.svelte.ts';
import { ANNOUNCE, FILE } from './copy.ts';
import { NODE_WRITE, type L } from './machine.ts';
import { OperateLink } from './operateLink.svelte.ts';
import type { Node } from './sample.ts';
import { imageRows } from './slots.ts';
import { LATEST_NAME, NO_TRACK, REFUSALS, UNKNOWN, appUnchanged, failureStage, fileTopologyOf, hubBack, refusalAnnounce, refusalOf, sameBytes, type Progress, type ReleaseFile } from './writeProgress.ts';

/** While a write or a recovery runs, the card checks stay as they are. */
const UPDATE_LOCKED: ReadonlySet<L> = new Set(['writing', 'hubRestarting', 'reconnectNeeded', 'stopped', 'recovering']);

export class Operate extends OperateLink {
  /**
   * What stands in the way of writing `bytes` to the checked nodes, before anything is sent: none checked, or every
   * checked node already on its app, its boot core and its recovery agent, compared as dagon-host's update_app,
   * update_core and update_agent do, against what the node answers now; what cannot be read is not called the same,
   * so the write goes and says why.
   */
  async noWrite(bytes: Uint8Array): Promise<'noneChecked' | { same: number; coreToo: boolean } | null> {
    const nodes = this.answering();
    if (nodes.length === 0) return 'noneChecked';
    const given = releaseVersions(bytes);
    const cores = releaseCoreVersions(bytes);
    const reach = this.reach;
    const at = (id: string) => reach.findIndex((n) => n.id === id);
    const first = given.get(at(nodes[0]));
    if (first === undefined) return null;
    let coreToo = false;
    for (const id of nodes) {
      const st = await this.statusOf(id);
      if (!st || !appUnchanged(bytes, at(id), st)) return null;
      const agent = releaseAgentDigest(bytes, at(id));
      if (agent && !sameBytes(st.status.slots[factorySlot()]?.digest16 ?? null, agent)) return null;
      const held = cores.get(at(id));
      if (!held) continue;
      if (!held.includes((await this.runningCore(id)) ?? NaN)) return null;
      coreToo = true;
    }
    return { same: first, coreToo };
  }

  /**
   * A release file was chosen ("パッケージファイルの選択", "応答するnノードへの
   * 優先書込", "別ファイルの選択"): dagon-host's release check against the
   * configuration on the canvas, before anything is sent.
   */
  takeFile(name: string, bytes: Uint8Array): void {
    if (!this.config) return;
    const ev = this.state === 'ready' ? 'chooseFile' : this.state === 'nodesSilent' ? 'updateResponding' : 'otherFile';
    this.go(ev);
    if (this.state !== 'reading') return;
    const file: ReleaseFile = { name, size: bytes.length, bytes, verdict: 'checking' };
    this.file = file;
    this.refusal = undefined;
    this.fileTopology = null;
    this.status = ANNOUNCE.fileChecking(name, FILE.size(bytes.length));
    this.go('read', false);
    try {
      check_release_for(bytes, topo_compile(this.config.yaml));
    } catch (e) {
      const k = refusalOf(e);
      this.file = { ...file, verdict: k };
      this.refusal = k;
      if (k === 'topology') this.fileTopology = fileTopologyOf(bytes, this.config.yaml);
      this.status = refusalAnnounce(k);
      return this.go(k === 'topology' ? 'topoDiff' : 'badSource');
    }
    this.file = { ...file, verdict: 'pass' };
    const given = releaseVersions(bytes);
    // topology node index = position in reach, as in update().
    this.updated.clear();
    this.fileVersions = Object.fromEntries(this.reach.flatMap((n, i) => (given.has(i) ? [[n.id, given.get(i)!]] : [])));
    this.rest = false;
    this.targets = this.answering();
    this.status = ANNOUNCE.fileOk(this.targets.length, this.config.nodes.length - this.reach.length);
    this.go('pass');
  }

  /** A node card's check: off leaves the node out of the next write (not while one runs). */
  toggleSkip(id: string): void {
    if (UPDATE_LOCKED.has(this.state)) return;
    const next = new Set(this.skip);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.skip = next;
  }

  /** The write's check of node `id` once a file is chosen (fileReady): an unchecked node is not written. Targets keep update order. */
  choose(id: string, on: boolean): void {
    if (this.state !== 'fileReady' || !this.reach.some((n) => n.id === id)) return;
    const chosen = new Set(this.targets);
    if (on) chosen.add(id);
    else chosen.delete(id);
    this.targets = this.reach.map((n) => n.id).filter((x) => chosen.has(x));
  }

  /** "nノードへの書込": dagon-host's apply on the checked nodes, the other nodes first and the node connected to the PC last. */
  async update(): Promise<void> {
    const c = this.config;
    const f = this.file;
    if (!c || !f || f.verdict !== 'pass' || this.state !== 'fileReady' || this.targets.length === 0) return;
    const my = this.session;
    this.go('update');
    this.stopWatching();
    // Every package written is kept first, so it can be written back later.
    this.keepFailed = this.deps.keep ? !(await this.deps.keep(f.bytes).catch(() => false)) : false;
    if (my !== this.session) return;
    this.running = 'update';
    this.startedAt = Date.now();
    this.failed = null;
    this.sentVersion = {};
    this.coreSent = {};
    this.wrote = new Set();
    this.firstWrite = 0;
    this.progress = Object.fromEntries(this.targets.map((id) => [id, { stage: 'waiting', percent: 0 }]));
    this.track = Object.fromEntries(this.targets.map((id) => [id, { ...NO_TRACK }]));
    // topology node index = position in reach (true for the sample, which follows its YAML).
    const reach = this.reach;
    const nodes = this.targets.map((id) => reach.findIndex((n) => n.id === id));
    let results: NodeResult[];
    try {
      results = await this.call((h) => h.apply(f.bytes, topo_compile(c.yaml), nodes, (e) => void (my === this.session && this.onEvent(e))));
    } catch {
      // The release passed the same check before, so only the link can fail here.
      if (my === this.session) this.lose();
      return;
    }
    if (my !== this.session) return;
    this.reconnectUntil = null;
    this.startedAt = null;
    this.finish(results);
  }

  private onEvent(e: UpdateEvent): void {
    const n = this.reach[e.node];
    if (!n) return;
    this.trackEvent(n.id, e);
    // Only the app's transfer carries the release's version; FAILED / REJECTED carry the version the node still runs.
    if (e.phase === PHASE.transfer && !e.bootCore) this.sentVersion[n.id] = e.version;
    if (e.phase === PHASE.transfer && e.bootCore) this.coreSent[n.id] = e.version;
    const was = this.progress[n.id] ?? { stage: 'waiting', percent: 0 };
    if (e.phase === PHASE.transfer || e.phase === PHASE.verify) {
      if (was.stage !== 'writing') this.status = ANNOUNCE.writing(n.name);
      this.progress[n.id] = { stage: 'writing', percent: e.phase === PHASE.verify ? 100 : e.percent, ...(e.bootCore ? { core: e.version } : {}) };
    } else if (e.phase === PHASE.reboot && this.isLocal(n)) {
      this.progress[n.id] = { stage: 'rebooting', percent: 100, ...(e.bootCore ? { core: e.version } : {}) };
      this.hubAway(this.sentVersion[n.id]);
    } else if (e.phase === PHASE.confirm && !e.bootCore) {
      // Only the app's confirmation finishes the node; its boot core's comes before it.
      this.progress[n.id] = { stage: e.wrote ? 'done' : 'unchanged', percent: 100 };
      this.updated.add(n.id);
      if (e.wrote) this.wrote.add(n.id);
      // The card shows the version the node now runs.
      const was = this.live[n.id];
      if (was?.hello) this.live[n.id] = { ...was, hello: { ...was.hello, activeVersion: e.version, trial: false } };
    }
  }

  /** Feeds node `id`'s write track (its one %) with one of dagon-host's events. */
  private trackEvent(id: string, e: UpdateEvent): void {
    const now = e.atMs;
    const k = this.track[id] ?? { ...NO_TRACK };
    if (e.phase === PHASE.transfer || e.phase === PHASE.rom) {
      // A new image (after a boot core's reboot) starts its own transfer.
      const fresh = k.t0 === null || k.xferMs !== null;
      this.track[id] = { ...k, shown: e.shown, ...(fresh ? { t0: now, xferMs: null, rebootAt: null, done: false } : {}) };
    } else if (e.phase === PHASE.verify) {
      this.track[id] = { ...k, shown: e.shown, xferMs: Math.max(1, now - (k.t0 ?? now)) };
    } else if (e.phase === PHASE.reboot) {
      // A recovery may reboot without a VERIFY: its transfer ends here.
      this.track[id] = { ...k, shown: e.shown, xferMs: k.xferMs ?? (k.t0 === null ? null : Math.max(1, now - k.t0)), rebootAt: now };
    } else if (e.phase === PHASE.confirm && e.bootCore) {
      // dagon-host counted the boot core's trial up to this moment, from the same moments, so its number is not under
      // what was shown; the clock stops here, or the page would run ahead of the number the next image starts from.
      this.track[id] = { ...k, shown: e.shown, t0: null, xferMs: null, rebootAt: null };
    } else if (e.phase === PHASE.confirm) {
      this.track[id] = { ...k, done: true };
    }
  }


  /** Where the run ended: all done, a node that stopped, or the node connected to the PC to choose again. */
  private finish(results: NodeResult[]): void {
    const reach = this.reach;
    for (const r of results) {
      const id = reach[r.node]?.id;
      if (id && r.outcome === 'notStarted') this.progress[id] = { stage: 'unwritten', percent: 0 };
      // The relaying node said the node answered nothing and dagon-host sent it nothing: its card takes the first write.
      if (id && r.outcome === 'needsFirstWrite') {
        this.progress[id] = { stage: 'silent', percent: 0 };
        this.live[id] = { ...(this.live[id] ?? UNKNOWN), responding: false };
      }
      // A confirmed node runs the new version now, also when a later node stops the run; one dagon-host took back
      // for a compatibility constraint (realigned) runs the version it had, confirmed.
      const hello = id ? this.live[id]?.hello : null;
      const settled = r.outcome === 'done' || r.outcome === 'realigned' || r.outcome === 'incompatible';
      if (id && hello && settled && r.version !== null) {
        this.live[id] = { ...this.live[id]!, hello: { ...hello, activeVersion: r.version, trial: false } };
      }
    }
    this.firstWrite = results.filter((r) => r.outcome === 'needsFirstWrite').length;
    const stop = results.find((r) => r.outcome === 'failed' || r.outcome === 'unknown');
    if (!stop) return this.done();
    const n = reach[stop.node];
    if (this.isLocal(n) && this.state === 'hubRestarting' && stop.failure?.reason === 'NO ANSWER') return this.autoFailed();
    const why = stop.failure;
    // A result this page does not name leaves the node unconfirmed: shown as a refusal, as an unnamed failure is.
    const f =
      stop.outcome === 'unknown' ? { stage: 'cannotFix' as const, reason: undefined } : why?.running != null ? { stage: 'previous' as const } : failureStage(why?.reason ?? 'NO ANSWER');
    // A stopped update has no "cannot fix" yet: an unnamed failure is a refusal of the update.
    this.stopAt(n.id, f.stage === 'cannotFix' ? 'refused' : f.stage, f.reason);
    if (why?.running != null) this.backRead = this.recordBack(n.id, why.running, why.reason);
  }

  /** Keeps what a write that went back left: the versions and reason at once, then what the node reports now. */
  private async recordBack(id: string, returned: number, reason: string): Promise<void> {
    const sent = this.sentVersion[id] ?? this.fileVersions[id];
    const n = this.reach.find((x) => x.id === id);
    if (sent === undefined || !n) return;
    const record: RollbackRecord = { node: id, failed_version: versionText(sent), returned_version: versionText(returned), reason, node_state: null };
    this.backRecord = record;
    try {
      const { hello, status } = await this.call((h) => h.status(this.at(n)));
      if (this.backRecord === record) this.backRecord = { ...record, node_state: { trial: hello.trial, last_reason: status.lastReason } };
    } catch {
      // A node that does not answer STATUS: the record says its state is not known.
    }
  }

  private stopAt(id: string, stage: Progress['stage'], reason?: string): void {
    const n = this.config!.nodes.find((x) => x.id === id)!;
    this.failed = id;
    this.progress[id] = { stage, percent: this.progress[id]?.percent ?? 0, reason };
    this.status = stage === 'previous' ? ANNOUNCE.previous(n.name) : stage === 'coreBack' ? ANNOUNCE.coreBack(n.name) : ANNOUNCE.stopped(n.name);
    this.go(this.state === 'reconnectNeeded' ? 'revertedOnReconnect' : 'nodeStuck');
    this.startWatching();
  }

  private done(): void {
    // An app left unchanged is on the package's version too.
    const v = Object.values(this.sentVersion)[0] ?? Object.values(this.fileVersions)[0];
    this.status = ANNOUNCE.done(v === undefined ? '' : versionText(v));
    this.finished = {
      kind: 'updated',
      left: this.reach.filter((n) => !this.updated.has(n.id) && !this.skip.has(n.id)).length,
      wrote: this.wrote.size,
      same: this.reach.filter((n) => this.updated.has(n.id) && !this.wrote.has(n.id)).length,
      ...(this.skip.size ? { leftOut: this.skip.size } : {}),
      ...(this.firstWrite ? { firstWrite: this.firstWrite } : {}),
    };
    this.go(this.state === 'hubRestarting' ? 'confirmed' : this.state === 'reconnectNeeded' ? 'reconnected' : 'doneNoHub');
    this.startWatching();
  }

  /**
   * "再接続" while the node connected to the PC reboots: the user chooses it again. While
   * dagon-host still reopens (hubRestarting) the chosen device goes under its next
   * reopen; after that (reconnectNeeded) dagon-host applies the confirm rule to it.
   * `onlyMine` (a device the browser reported connected, nobody chose it):
   * taken only when it is the node connected to the PC on trial with the version just sent.
   * Resolves whether the device was taken.
   */
  async reconnectHub(pick = this.deps.reselect, onlyMine = false): Promise<boolean> {
    if (!pick || !this.link || (this.state !== 'hubRestarting' && this.state !== 'reconnectNeeded')) return false;
    let t: Transport;
    try {
      t = await pick();
    } catch {
      return false;
    }
    // hubRestarting: dagon-host is still reopening (its apply holds the queue), so the device goes under its next reopen now.
    if (this.state === 'hubRestarting') {
      this.swap(t);
      return true;
    }
    // reconnectNeeded: swapping, checking and confirming run as one queued step, so two devices reported at once cannot interleave.
    const link = this.link;
    const r = await this.call(async (h) => {
      const old = this.transport;
      link.use(t);
      try {
        const why = this.notThisHub(await h.hello(LOCAL), onlyMine);
        if (why) throw why;
        const confirmed = await h.confirm(LOCAL, false);
        const core = await h.bootCoreStatus(LOCAL).catch(() => null);
        const st = await h.status(LOCAL).catch(() => null);
        const hubAt = this.reach.findIndex((n) => this.isLocal(n));
        const appSame = !!st && !!this.file && appUnchanged(this.file.bytes, hubAt, st);
        this.swap(t, old);
        return { confirmed, core: core ? core.slots[core.active].version : null, appSame };
      } catch (e) {
        this.noteWhy(isNote(e) ? e : { kind: 'silent' });
        if (old) link.use(old);
        return null;
      }
    }).catch(() => null);
    if (!r) {
      // Never left open while 切断 is held: the dialog stays, with the reason under 詳細表示.
      void t.close().catch(() => undefined);
      this.alert = ANNOUNCE.reconnectTimeout;
      return false;
    }
    this.confirmed(r.confirmed, r.core, r.appSame);
    return true;
  }

  /** reconnectNeeded: the node connected to the PC confirmed on the version just sent goes on to done; on its previous version it stops there (stopped). */
  private confirmed(r: ConfirmResult, core: number | null, appSame: boolean): void {
    const hub = this.reach.find((n) => this.isLocal(n))!;
    const stage = hubBack(r, this.sentVersion[hub.id], this.coreSent[hub.id], core, appSame);
    this.alert = '';
    if (stage === 'coreBack' || stage === 'unwritten') return this.stopAt(hub.id, stage);
    if (stage === 'previous') {
      this.stopAt(hub.id, 'previous');
      this.backRead = this.recordBack(hub.id, r.version, 'ROLLED BACK');
      return;
    }
    this.progress[hub.id] = { stage: 'done', percent: 100 };
    if (this.running === 'update') {
      this.updated.add(hub.id);
      this.wrote.add(hub.id);
      return this.done();
    }
    this.finishRun(hub, r.version);
    this.status = ANNOUNCE.done(versionText(r.version));
    this.go('reconnected');
    this.startWatching();
  }

  /** The node a write names without an id: the stopped one (stopped, recoverFailed) or the first silent one (nodesSilent). */
  get recoverTarget(): Node | undefined {
    const id = this.state === 'recoverFailed' ? this.recovering : this.state === 'stopped' ? this.failed : this.stuck?.id;
    return this.reach.find((n) => n.id === id);
  }

  private async latest(): Promise<Uint8Array | null> {
    return (await this.deps.latest?.().catch(() => null)) ?? null;
  }

  /** "最新リリースの使用": the project's latest release, checked like a chosen file. */
  async useLatest(): Promise<boolean> {
    const bytes = await this.latest();
    if (!bytes) {
      this.alert = ANNOUNCE.noRelease;
      this.latestMissing = true;
      return false;
    }
    this.takeFile(LATEST_NAME, bytes);
    return this.file?.verdict === 'pass';
  }

  /**
   * 「書込」 on one node (`id`, else the stuck one): the normal transfer
   * when it answers; silent, `direct` without a ROM entry and, with one,
   * `confirm` until `confirmed`, nothing touched before that. Takes the
   * release on the screen (else the project's latest) and, for the ROM path,
   * the project's boot core. Without any release it asks for one
   * (`needFile`); `chosen` is that file.
   */
  async writeNode(o: { id?: string; chosen?: { name: string; bytes: Uint8Array }; confirmed?: boolean } = {}): Promise<'needFile' | 'direct' | 'confirm' | void> {
    const c = this.config;
    const chosen = o.chosen;
    const n = o.id ? this.reach.find((x) => x.id === o.id) : this.recoverTarget;
    if (!c || !n || !NODE_WRITE.has(this.state)) return;
    const topo = topo_compile(c.yaml);
    const index = this.reach.indexOf(n);
    // A link that fails here fails the write itself, with its reason.
    const way = await this.call((h) => h.writePath(index, topo)).catch(() => 'transfer' as const);
    if (way === 'direct') return 'direct';
    let release = chosen?.bytes ?? (this.file?.verdict === 'pass' ? this.file.bytes : null);
    if (chosen) {
      try {
        check_release_for(chosen.bytes, topo);
      } catch (e) {
        this.alert = refusalAnnounce(refusalOf(e));
        return;
      }
      this.file = { name: chosen.name, size: chosen.bytes.length, bytes: chosen.bytes, verdict: 'pass' };
    }
    release ??= await this.latest();
    if (!release) return 'needFile';
    const bytes = release;
    if (way === 'rom' && !o.confirmed) {
      // A release the host would refuse is not worth the question: the refusal comes with its reason, nothing touched.
      try {
        check_release_for(bytes, topo);
        return 'confirm';
      } catch {
        // Below.
      }
    }
    const bootcore = (await this.deps.bootcore?.(n.id).catch(() => null)) ?? null;
    const stage0 = (await this.deps.stage0?.(n.id).catch(() => null)) ?? null;
    await this.runRecovery(n, 'recover', (h, on) => h.recover(index, bytes, topo, bootcore, stage0, !!o.confirmed, on));
  }

  /** "再試行" (recoverFailed): the operation that failed, again (a rollback stays a rollback). */
  async retry(): Promise<'needFile' | 'direct' | 'confirm' | void> {
    if (this.state !== 'recoverFailed') return;
    if (this.lastKind === 'rollback' && this.recovering) return this.rollback(this.recovering);
    return this.writeNode();
  }

  /** "前のバージョンへの復元" (the detail of a node, and the trial choice): the version in the other slot. */
  async rollback(id: string): Promise<void> {
    const reach = this.reach;
    const n = reach.find((x) => x.id === id);
    if (!n) return;
    const index = reach.indexOf(n);
    await this.runRecovery(n, 'rollback', async (h, on) => {
      const v = await h.rollback(this.at(n), index, imageRows(n.mcuId), on);
      return { path: 'retransfer', version: v, rom: null };
    });
  }

  /** "このバージョンで確定する": keeps the version a node runs on trial. */
  async keepTrial(id: string): Promise<void> {
    const n = this.reach.find((x) => x.id === id);
    if (!n || !this.host) return;
    try {
      await this.call((h) => h.confirm(this.at(n), true));
    } catch {
      // The next watch round shows whether it still answers.
    }
    await this.refreshStatus(id);
  }

  /** What ran last in recovering, so "再試行" repeats it. */
  private lastKind: 'recover' | 'rollback' = 'recover';

  private async runRecovery(
    n: Node,
    kind: 'recover' | 'rollback',
    run: (h: Host, on: (e: UpdateEvent) => void) => Promise<RecoverResult>,
  ): Promise<void> {
    const my = this.session;
    this.go(this.state === 'recoverFailed' ? 'recoverAgain' : kind);
    if (this.state !== 'recovering') return;
    this.stopWatching();
    this.lastKind = kind;
    this.recovering = n.id;
    this.recovered = null;
    this.running = kind;
    this.startedAt = Date.now();
    this.progress = { ...this.progress, [n.id]: { stage: 'writing', percent: 0 } };
    this.track = { ...this.track, [n.id]: { ...NO_TRACK } };
    const on = (e: UpdateEvent) => {
      if (my !== this.session || this.reach[e.node]?.id !== n.id) return;
      this.trackEvent(n.id, e);
      if (e.phase === PHASE.rom) this.progress[n.id] = { stage: 'lastResort', percent: e.percent };
      else if (e.phase === PHASE.transfer) this.progress[n.id] = { stage: 'writing', percent: e.percent };
      // TRANSFER and REBOOT carry the version the node is given (FAILED carries the one it runs).
      if ((e.phase === PHASE.transfer || e.phase === PHASE.reboot) && !e.bootCore) this.sentVersion[n.id] = e.version;
      // The node connected to the PC reboots onto it: the same reconnect and confirm rule as an update (hubRestarting / reconnectNeeded).
      if (e.phase === PHASE.reboot && this.isLocal(n)) {
        this.progress[n.id] = { stage: 'rebooting', percent: 100 };
        this.hubAway(this.sentVersion[n.id]);
      }
    };
    let r: RecoverResult;
    try {
      r = await this.call((h) => run(h, on));
    } catch (e) {
      if (my !== this.session) return;
      this.reconnectUntil = null;
      this.startedAt = null;
      return this.recoveryFailed(n, e instanceof Error ? e.message : '');
    }
    if (my !== this.session) return;
    this.reconnectUntil = null;
    this.startedAt = null;
    this.recovered = r;
    this.progress[n.id] = { stage: 'done', percent: 100 };
    this.track[n.id] = { ...(this.track[n.id] ?? NO_TRACK), done: true };
    this.finishRun(n, r.version);
    this.live[n.id] = { ...(this.live[n.id] ?? UNKNOWN), responding: true, lastSeen: Date.now() };
    const rest = this.file?.verdict === 'pass' ? Object.entries(this.progress).filter(([, p]) => p.stage === 'unwritten').map(([id]) => id) : [];
    this.status = kind === 'rollback' ? ANNOUNCE.rolledBack(n.name, versionText(r.version)) : ANNOUNCE.recovered(n.name, rest.length);
    // The PC-connected node's REBOOT event moved the state on while dagon-host ran.
    if ((this.state as L) === 'hubRestarting') {
      this.go('confirmed');
    } else if (rest.length) {
      this.targets = rest;
      this.rest = true;
      this.go('fixedRest');
    } else {
      this.go('fixedNoRest');
    }
    this.startWatching();
    await this.refreshStatus(n.id);
  }

  /** A recovery or rollback got through on `version`: what done says. */
  private finishRun(n: Node, version: number): void {
    if (this.running === 'recover' && this.file?.verdict === 'pass') this.updated.add(n.id);
    const v = versionText(version);
    this.finished = this.running === 'rollback' ? { kind: 'rolledBack', name: n.name, version: v } : { kind: 'recovered', name: n.name, version: v };
  }

  /** A recovery or rollback that did not get through, shown with dagon-host's reason. */
  private recoveryFailed(n: Node, reason: string): void {
    // The rebooted PC-connected node was not reopened in time: the user chooses it again (reconnectNeeded).
    if (this.state === 'hubRestarting' && reason === 'NO ANSWER') return this.autoFailed();
    const percent = this.progress[n.id]?.percent ?? 0;
    this.progress[n.id] = { ...failureStage(reason), percent };
    this.status = REFUSALS.includes(reason) ? refusalAnnounce(reason as Refusal) : ANNOUNCE.stopped(n.name);
    this.go(this.state === 'hubRestarting' ? 'nodeStuck' : 'gaveUp');
  }
}
