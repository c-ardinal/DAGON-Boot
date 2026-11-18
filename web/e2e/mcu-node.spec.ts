import { expect, test } from '@playwright/test';
import { openSample } from './console.ts';

test('a node placed by MCU shows the flash its definition lays out, read-only, and asks for no board or flash', async ({ page }) => {
  await openSample(page);
  await page.getByRole('tab', { name: 'マイコン', exact: true }).click();
  await page.locator('[aria-label="ボード 配置"]').filter({ hasText: 'RP2040' }).first().dblclick();
  const inspector = page.locator('section.inspector');
  const own = inspector.locator('[data-field="flash-own"]');
  await expect(own).toHaveText(/^\d+ (KiB|MiB)$/);
  await expect(inspector.getByText('フラッシュ（定義の配置）', { exact: true })).toBeVisible();
  await expect(inspector.locator('select[data-field="board.flash"], [data-field="flash-own"]:is(input, select)')).toHaveCount(0);
  await expect(inspector.getByText('ボード 未設定')).toHaveCount(0);
});
