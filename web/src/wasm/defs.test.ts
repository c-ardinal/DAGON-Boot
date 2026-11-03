import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import { mcuConnection, supportedBoards, useDefinitions } from './dagon.ts';

const text = (rel: string) => new TextDecoder().decode(readFileSync(new URL(`../../../${rel}`, import.meta.url)));
const mcu = (id: string, files: [string, string][] = []) => ({ label: id, toml: text(`targets/mcu/${id}/dagon-mcu.toml`), files });
const board = (id: string) => ({ label: id, toml: text(`targets/boards/${id}/dagon-board.toml`) });

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

  test('none, or ones that do not load, keep the builtin set', () => {
    expect(useDefinitions(null)).toBe(false);
    expect(useDefinitions('{"mcus": []}')).toBe(false);
    expect(useDefinitions(served.replace('sector = 0x1000', 'sector = 0x80'))).toBe(false);
    expect(supportedBoards().map((b) => b.id)).not.toContain('desk-pico');
  });

  test("the project's own board joins the table once they load", () => {
    expect(useDefinitions(served)).toBe(true);
    expect(supportedBoards().find((b) => b.id === 'desk-pico')).toMatchObject({ product: 'Desk Pico', mcu: 'rp2040', mcuName: 'RP2040' });
  });
});
