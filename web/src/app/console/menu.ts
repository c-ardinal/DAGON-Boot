// The ☰ menu: only operations that run here; a group's items by how often they are used.
import type { MenuGroup } from '../../screens/Menu.svelte';
import { AGENT, CONFIRM, DEFINE, HELP, HUB_TOGGLE, MENU, PARTS, START, ZOOM } from '../copy.ts';
import type { Define } from '../define/define.svelte.ts';
import type { StartChoice } from '../start.svelte.ts';

/** What the menu reads and does; each item's `off` says why it cannot be used now. */
export interface MenuContext {
  d: Define;
  mode: 'operate' | 'define';
  /** A write or its recovery is under way. */
  busy: boolean;
  /** No project is open in serve (development stand-ins, the start screen). */
  noProject: boolean;
  demoSite: boolean;
  simulated: boolean;
  agentLinked: boolean;
  /** The header is too narrow for the view toggle, so the menu holds it. */
  barNarrow: boolean;
  partsOpen: boolean;
  arranged: boolean;
  /** A node is selected on the canvas (its card can be moved). */
  nodeSelected: boolean;
  /** The DAGON mark is on the page. */
  markShown: boolean;
  connected: boolean;
  /** Why 接続 / 切断 cannot be used now. */
  hubLocked: string | null;
  /** Nothing runs on the node connected to the PC. */
  idle: boolean;
  /** Why the selected node cannot go back, null when it can, undefined with no node selected. */
  rollbackOff: string | null | undefined;
  /** The last proposal not yet taken in, or -1. */
  lastProposal: number;
  hasNotes: boolean;
  motionStill: boolean;
  run: {
    toStart(c: StartChoice): void;
    save(): void;
    restart(): void;
    openParts(): void;
    arrange(): void;
    moveCard(): void;
    moveMark(): void;
    fit(): void;
    toggleHub(): void;
    chooseFile(): void;
    rollback(): void;
    talk(): void;
    applyProposal(): void;
    link(): void;
    cli(): void;
    unlink(): void;
    help(keys: boolean): void;
    copyLog(): void;
    copyUrl(): void;
    toggleMotion(): void;
    confirm(c: { title: string; sentence: string; label: string }, run: () => void): void;
  };
}

export function consoleMenu(c: MenuContext): MenuGroup[] {
  const { d, run } = c;
  const notIdle = c.busy ? MENU.off.notIdle : null;
  const notDefine = c.mode !== 'define' ? MENU.off.notDefine : null;
  const graph = d.view === 'graph';
  return [
    {
      label: MENU.groups.project,
      items: [
        { label: START.new, hint: MENU.hint.new, control: 'project', off: notIdle, run: () => run.toStart('new') },
        { label: START.open, hint: MENU.hint.open, control: c.demoSite ? 'open' : 'project', off: notIdle, run: () => run.toStart('open') },
        { label: MENU.save, hint: MENU.hint.save, off: c.noProject ? MENU.off.noProject : null, run: run.save },
        { label: MENU.restart, hint: MENU.hint.restart, control: 'project', danger: true, off: notIdle, run: run.restart },
      ],
    },
    {
      label: MENU.groups.define,
      items: [
        { label: DEFINE.undo, hint: MENU.hint.undo, key: 'Ctrl+Z', control: 'edit', off: notDefine ?? (d.canUndo ? null : MENU.off.noUndo), run: () => d.undo() },
        { label: DEFINE.redo, hint: MENU.hint.redo, key: 'Ctrl+Shift+Z', control: 'edit', off: notDefine ?? (d.canRedo ? null : MENU.off.noRedo), run: () => d.redo() },
        { label: PARTS.open, hint: MENU.hint.parts, off: notDefine ?? (c.partsOpen ? MENU.off.partsOpen : null), run: run.openParts },
        { label: MENU.arrange, hint: MENU.hint.arrange, off: c.arranged ? null : MENU.off.arranged, run: run.arrange },
        // The single-pointer way to move a card (WCAG 2.5.7): the next click on the canvas places it.
        { label: MENU.moveCard, hint: MENU.hint.moveCard, key: 'Alt+矢印', off: (c.mode === 'define' && !graph ? MENU.off.notGraph : null) ?? (c.nodeSelected ? null : MENU.off.noNode), run: run.moveCard },
        // The view toggle leaves the menu while the header holds it.
        ...(c.barNarrow ? [{ label: graph ? MENU.viewTable : MENU.viewGraph, hint: graph ? MENU.hint.viewTable : MENU.hint.viewGraph, off: notDefine, run: () => (d.view = graph ? 'table' : 'graph') }] : []),
        { label: ZOOM.fit, hint: MENU.hint.fit, key: 'F', off: c.mode === 'define' && !graph ? MENU.off.notGraph : null, run: run.fit },
      ],
    },
    {
      label: MENU.groups.connect,
      items: [
        { label: c.connected ? HUB_TOGGLE.disconnect : HUB_TOGGLE.connect, hint: c.connected ? MENU.hint.disconnect : MENU.hint.connect, control: 'connect', off: c.hubLocked, run: run.toggleHub },
        { label: MENU.place, hint: MENU.hint.place, control: 'file', off: !c.connected ? MENU.off.notConnected : c.idle ? null : MENU.off.notIdle, run: run.chooseFile },
        {
          label: MENU.rollback,
          hint: MENU.hint.rollback,
          control: 'rollback',
          off: !c.connected ? MENU.off.notConnected : !c.idle ? MENU.off.notIdle : c.rollbackOff === undefined ? MENU.off.noNode : c.rollbackOff,
          run: run.rollback,
        },
      ],
    },
    {
      label: MENU.groups.dagon,
      items: [
        ...(c.agentLinked
          ? [
              { label: AGENT.menuTalk, hint: MENU.hint.talk, control: 'ask', run: run.talk },
              { label: MENU.applyProposal, hint: MENU.hint.apply, control: 'apply', off: c.lastProposal < 0 ? MENU.off.noProposal : null, run: run.applyProposal },
            ]
          : [{ label: AGENT.menuLink, hint: MENU.hint.link, control: 'link', off: c.simulated ? MENU.off.demo : null, run: run.link }]),
        ...(c.markShown ? [{ label: MENU.moveMark, hint: MENU.hint.moveMark, key: 'Alt+矢印', run: run.moveMark }] : []),
        { label: MENU.cli, hint: MENU.hint.cli, off: c.simulated ? MENU.off.demo : null, run: run.cli },
        ...(c.agentLinked ? [{ label: AGENT.menuUnlink, hint: MENU.hint.unlink, danger: true, run: () => run.confirm(CONFIRM.unlink, run.unlink) }] : []),
      ],
    },
    {
      label: MENU.groups.help,
      items: [
        { label: HELP.heading, hint: MENU.hint.help, run: () => run.help(false) },
        { label: MENU.keys, hint: MENU.hint.keys, run: () => run.help(true) },
        { label: MENU.copyLog, hint: MENU.hint.copyLog, off: c.hasNotes ? null : MENU.off.noLog, run: run.copyLog },
        { label: MENU.copyUrl, hint: MENU.hint.copyUrl, run: run.copyUrl },
        { label: MENU.motion, hint: c.motionStill ? MENU.hint.motionResume : MENU.hint.motionStop, run: run.toggleMotion },
      ],
    },
  ];
}
