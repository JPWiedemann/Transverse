import { expect, test, type Page } from '@playwright/test';

// The built site in a real browser: wasm module, Web Worker, form, all tabs. The numbers are the
// reference values of the project (default stack-up: Z0 = 35.001 ohm, synthesis 35 ohm -> w = 0.3605 mm, from
// the grid solver; the finite elements of the app give 34.98 ohm).

/** Fail a test on errors in the page (uncaught exceptions, console errors). */
function watchErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  return errors;
}

const status = (page: Page) => page.getByRole('status');
const tab = (page: Page, name: string) => page.getByRole('tab', { name, exact: true });

async function analyze(page: Page) {
  await page.getByRole('button', { name: 'Analyze', exact: true }).click();
  await expect(status(page)).toContainText('Analysis done', { timeout: 60_000 });
}

test.beforeEach(async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('button', { name: 'Analyze', exact: true })).toBeVisible({ timeout: 30_000 });
});

test('starts on the cross-section, with the tabs in the order of the desktop app', async ({ page }) => {
  await expect(page.getByRole('tab')).toHaveText(['Cross-section', 'Results', 'Field', 'Return current', 'Loss vs f']);
  await expect(tab(page, 'Cross-section')).toHaveAttribute('aria-selected', 'true');
  // a run leads from the sketch to the results, any other tab stays
  await analyze(page);
  await expect(tab(page, 'Results')).toHaveAttribute('aria-selected', 'true');
  await tab(page, 'Loss vs f').click();
  await analyze(page);
  await expect(tab(page, 'Loss vs f')).toHaveAttribute('aria-selected', 'true');
});

test('analyzes the default stack-up', async ({ page }) => {
  const errors = watchErrors(page);
  await analyze(page);
  await expect(status(page)).toContainText(/Z0 = 34\.98/);
  await expect(page.locator('.card', { hasText: 'Z0' }).first()).toContainText('34.98');
  await expect(page.locator('.report')).toContainText('Ground split');
  expect(errors).toEqual([]);
});

test('every tab shows its view', async ({ page }) => {
  const errors = watchErrors(page);
  await analyze(page);

  await tab(page, 'Cross-section').click();
  await expect(page.getByRole('img', { name: /Cross-section of the line/ })).toBeVisible();

  await tab(page, 'Return current').click();
  await expect(page.getByRole('img', { name: /Return current density/ })).toBeVisible();
  await expect(page.locator('table.split')).toContainText('Ground plane 1');

  await tab(page, 'Loss vs f').click();
  const loss = page.getByRole('img', { name: /loss versus frequency/ });
  await expect(loss).toBeVisible();
  await expect(loss.locator('path.total')).toHaveAttribute('d', /^M/);
  // the readout at the cursor comes from the module
  const box = (await loss.boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await expect(page.locator('.readout')).toContainText('Total:');

  await tab(page, 'Field').click();
  await expect(page.getByRole('img', { name: 'Field in the cross-section' })).toBeVisible();
  // the colour bar appears once the image is rendered
  await expect(page.locator('.bar canvas')).toBeVisible({ timeout: 20_000 });
  for (const mode of ['potential', 'permittivity', 'mesh', 'e_linear']) {
    await page.getByLabel('Field view').selectOption(mode);
  }
  await page.getByLabel('Field view').selectOption('e_db_lines');
  const field = page.getByRole('img', { name: 'Field in the cross-section' });
  const fbox = (await field.boundingBox())!;
  await page.mouse.move(fbox.x + fbox.width / 2, fbox.y + fbox.height / 2);
  await expect(page.locator('.readout')).toContainText('V =');
  expect(errors).toEqual([]);
});

test('synthesizes the trace width for 35 ohm', async ({ page }) => {
  await page.locator('#p-w').fill('0.5');
  await page.getByLabel('Target Z0').fill('35');
  await page.getByRole('button', { name: 'Synthesize', exact: true }).click();
  await expect(status(page)).toContainText('Synthesis done', { timeout: 60_000 });
  const w = Number(await page.locator('#p-w').inputValue());
  expect(Math.abs(w / 0.3605 - 1)).toBeLessThan(3e-3);
});

test('synthesis with a phase also sets the line length', async ({ page }) => {
  await page.getByLabel('Frequency', { exact: true }).fill('2.4');
  await page.getByLabel('Target Z0').fill('35');
  // empty phase keeps the length
  await page.getByRole('button', { name: 'Synthesize', exact: true }).click();
  await expect(status(page)).toContainText('Synthesis done', { timeout: 60_000 });
  await expect(page.locator('#p-length')).toHaveValue('100');
  // with a phase the length follows: a quarter wavelength, 66.004 / 4 mm for this stack-up at 2.4 GHz
  await page.getByLabel('Target phase').fill('90');
  await page.getByRole('button', { name: 'Synthesize', exact: true }).click();
  await expect(status(page)).toContainText('Synthesis done', { timeout: 60_000 });
  const length = Number(await page.locator('#p-length').inputValue());
  expect(Math.abs(length / (66.0036 / 4) - 1)).toBeLessThan(1e-3);
  await expect(page.locator('.card', { hasText: 'Electrical length' })).toContainText('90.00');
  await expect(page.locator('.card', { hasText: 'Wavelength' })).toContainText('66.0');
  // an analysis keeps the length and calculates the phase
  await page.getByRole('button', { name: 'Analyze', exact: true }).click();
  await expect(status(page)).toContainText('Analysis done', { timeout: 60_000 });
  await expect(page.locator('.card', { hasText: 'Electrical length' })).toContainText('90.00');
  // a bad phase is reported and nothing runs
  await page.getByLabel('Target phase').fill('abc');
  await page.getByRole('button', { name: 'Synthesize', exact: true }).click();
  await expect(status(page)).toContainText('Target phase');
});

test('cancel stops a long solve and the page keeps working', async ({ page }) => {
  await page.locator('#p-tol').fill('0.0001'); // a very tight target error makes the solve long
  await page.getByRole('button', { name: 'Analyze', exact: true }).click();
  await expect(status(page)).toContainText(/Refinement|Meshing/, { timeout: 30_000 });
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(status(page)).toContainText('Cancelled');
  await page.locator('#p-tol').fill('0.5');
  await analyze(page);
  await expect(status(page)).toContainText(/Z0 = 34\.98/);
  // the cancel threw the old result away only until the new analysis finished
  await tab(page, 'Loss vs f').click();
  await expect(page.getByRole('img', { name: /loss versus frequency/ })).toBeVisible();
});

test('shows invalid input at the field and does not start', async ({ page }) => {
  await page.locator('#p-w').fill('abc');
  await page.getByRole('button', { name: 'Analyze', exact: true }).click();
  await expect(status(page)).toContainText("Trace width w: 'abc' is not a number");
  await expect(page.locator('#p-w')).toHaveClass(/invalid/);
});

test('remembers the form, unit and theme after a reload, but not the tab', async ({ page }) => {
  await page.locator('#p-w').fill('0.42');
  await page.getByLabel('Trace width w unit').selectOption('mil');
  await tab(page, 'Loss vs f').click();
  await page.getByLabel('Theme').selectOption('dark');
  await page.reload();
  await expect(page.getByRole('button', { name: 'Analyze', exact: true })).toBeVisible({ timeout: 30_000 });
  await expect(page.getByLabel('Trace width w unit')).toHaveValue('mil');
  expect(Number(await page.locator('#p-w').inputValue())).toBeGreaterThan(0);
  await expect(tab(page, 'Cross-section')).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
});

test('fields that do not apply are hidden', async ({ page }) => {
  await expect(page.locator('#p-dmin')).toHaveCount(0);
  await page.getByLabel('Mode', { exact: true }).selectOption({ label: 'Manual' });
  await expect(page.locator('#p-dmin')).toBeVisible();

  // a hidden field with a broken value cannot block the analysis, and gets its text back when it returns
  await page.locator('#p-s').fill('abc');
  await page.getByRole('button', { name: 'Analyze', exact: true }).click();
  await expect(status(page)).toContainText("Gap s: 'abc' is not a number");
  await page.getByLabel('Mode', { exact: true }).selectOption({ label: 'Auto (target error)' });
  await page.getByLabel('Coplanar ground').uncheck();
  await expect(page.locator('#p-s')).toHaveCount(0);
  await analyze(page);
  await expect(page.locator('.report')).toContainText('stripline');
  // the gap cannot be synthesised without a coplanar ground
  await expect(page.getByLabel('Synthesize', { exact: false }).first().locator('option')).toHaveText(['w']);
  await page.getByLabel('Coplanar ground').check();
  await expect(page.locator('#p-s')).toHaveValue('abc');

  await page.getByLabel('Upper half (else air above)').uncheck();
  await expect(page.locator('#p-h1')).toHaveCount(0);
  await expect(page.getByText('Upper half', { exact: true })).toHaveCount(0);
});

test('presets set the structure and keep the dimensions', async ({ page }) => {
  const errors = watchErrors(page);
  await page.locator('#p-w').fill('0.5');
  const presets = page.getByRole('group', { name: 'Presets' });
  for (const [button, name] of [
    ['CPWG, outer layer', 'grounded coplanar waveguide'],
    ['CPW, no planes', 'coplanar waveguide'],
    ['Stripline', 'stripline'],
    ['Microstrip', 'microstrip'],
    ['CPWG, inner layer', 'grounded coplanar waveguide'],
  ] as const) {
    await presets.getByRole('button', { name: button, exact: true }).click();
    await expect(page.locator('#p-w')).toHaveValue('0.5');
    await analyze(page);
    await expect(page.locator('.report')).toContainText(name);
    await tab(page, 'Cross-section').click();
    await expect(page.getByRole('img', { name: /Cross-section of the line/ })).toBeVisible();
    await tab(page, 'Return current').click();
    await expect(page.getByRole('img', { name: /Return current density/ })).toBeVisible();
    await tab(page, 'Field').click();
    await expect(page.locator('.bar canvas')).toBeVisible({ timeout: 20_000 });
    await tab(page, 'Results').click();
  }
  expect(errors).toEqual([]);
});

test('the field views draw the triangle mesh', async ({ page }) => {
  const errors = watchErrors(page);
  await expect(page.getByLabel('Solver', { exact: true })).toHaveCount(0);
  await analyze(page);
  await expect(page.locator('.report')).toContainText('finite elements');
  await tab(page, 'Field').click();
  await expect(page.locator('.bar canvas')).toBeVisible({ timeout: 20_000 });
  await page.getByLabel('Mesh overlay').check();
  await expect(page.locator('path.tri')).toHaveAttribute('d', /^M/, { timeout: 20_000 });
  await expect(page.locator('p.title')).toContainText('finite elements');
  // the image, the readout and the grid-less views still work on the mesh
  const field = page.getByRole('img', { name: 'Field in the cross-section' });
  const box = (await field.boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await expect(page.locator('.readout')).toContainText('V =');
  await page.getByLabel('Field view').selectOption('mesh');
  await expect(page.locator('path.tri')).toHaveAttribute('d', /^M/);
  expect(errors).toEqual([]);
});
