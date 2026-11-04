import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import { mcuConnection, supportedBoards, useDefinitions } from './dagon.ts';

const text = (rel: string) => new TextDecoder().decode(readFileSync(new URL(`../../../${rel}`, import.meta.url)));
const mcu = (id: string, files: [string, string][] = []) => ({ label: id, toml: text(`targets/mcu/${id}/dagon-mcu.toml`), files });
const board = (id: string) => ({ label: id, toml: text(`targets/boards/${id}/dagon-board.toml`) });

/** The builtin set: the id of every board package of the tree (as a set; order carries no meaning). */
const expectBuiltin = (ids: string[]) => {
  const want = readdirSync(new URL('../../../targets/boards/', import.meta.url), { withFileTypes: true })
    .filter((d) => d.isDirectory() && existsSync(new URL(`../../../targets/boards/${d.name}/dagon-board.toml`, import.meta.url)))
    .map((d) => /^id = "([^"]+)"/m.exec(text(`targets/boards/${d.name}/dagon-board.toml`))?.[1] ?? d.name);
  expect([...ids].sort()).toEqual(want.sort());
};

/** What GET /v1/definitions sends for the builtin set plus a project's own board. */
const served = JSON.stringify({
  mcus: [mcu('rp2040'), mcu('stm32f401re'), mcu('esp32s3', [['partitions.csv', text('targets/mcu/esp32s3/partitions.csv')]])],
  boards: [
    board('waveshare-rp2040-touch-lcd-1.28'),
    board('nucleo-f401re'),
    board('m5stopwatch'),
    { label: 'dagon.toml [board.desk-pico]', toml: 'id = "desk-pico"\nproduct = "Desk Pico"\nmcu = "rp2040"\nflash_bytes = 0x400000\ndrawing = "pico"\ndefault_link = { kind = "i2c", addr = 0x44, control = "full" }\n' },
  ],
});

describe('definitions from dagon serve', () => {
  test('the write panel says how to connect each MCU, as it always has', () => {
    expect(mcuConnection('rp2040')).toEqual({ label: 'USB（BOOT を押しながら）', hint: 'BOOT を押しながら USB をつないでから、書き込んでください。' });
    expect(mcuConnection('stm32f401re')).toEqual({ label: 'ST-LINK', hint: 'ST-LINK を USB でつないでから、書き込んでください。' });
    expect(mcuConnection('esp32s3')).toEqual({ label: 'USB-C', hint: 'USB-C でつないでから、書き込んでください（つながらないときは BOOT を押しながら）。' });
    expect(mcuConnection('nope')).toBeNull();
  });

  test('none (no serve, an older serve) is the builtin set', () => {
    expect(useDefinitions(null)).toBe('builtin');
    expectBuiltin(supportedBoards().map((b) => b.id));
    expect(mcuConnection('rp2040')).not.toBeNull();
  });

  test("a project's definitions that fail block the page: no MCU or board, never the builtin set", () => {
    const blocked = () => {
      expect(supportedBoards()).toEqual([]);
      expect(mcuConnection('rp2040')).toBeNull();
    };
    // serve said 500 (DEFS_ERROR)
    expect(useDefinitions(null, true)).toBe('blocked');
    blocked();
    // what serve sent does not load
    expect(useDefinitions('{"mcus": []}')).toBe('blocked');
    blocked();
    expect(useDefinitions(served.replace('sector = 0x1000', 'sector = 0x80'))).toBe('blocked');
    blocked();
  });

  test("the project's own board joins the table once they load", () => {
    expect(useDefinitions(served)).toBe('loaded');
    expect(supportedBoards().find((b) => b.id === 'desk-pico')).toMatchObject({ product: 'Desk Pico', mcu: 'rp2040', mcuName: 'RP2040' });
  });

  test('the next project without definitions is back on the builtin set; one whose definitions fail on none', () => {
    const ids = () => supportedBoards().map((b) => b.id);
    expect(useDefinitions(served)).toBe('loaded');
    expect(useDefinitions(null)).toBe('builtin');
    expectBuiltin(ids());
    expect(useDefinitions(served)).toBe('loaded');
    expect(useDefinitions('{"mcus": []}')).toBe('blocked');
    expect(ids()).toEqual([]);
    expect(useDefinitions(null)).toBe('builtin');
    expectBuiltin(ids());
  });
});
