import { expect, test } from '@playwright/test';
import { canvasState, openSample, settled } from './console.ts';

test('the first view is the explicit fit: pressing it on an untouched canvas changes nothing', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  const first = await canvasState(page);
  await page.getByRole('button', { name: 'キャンバス全体を表示する' }).click();
  await settled(page);
  expect(await canvasState(page)).toEqual(first);
});
