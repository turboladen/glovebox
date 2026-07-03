import { test, expect, type Page } from '@playwright/test'

// TP-18: Parts Tab (slotless — parts carry a plain-text location)
test.describe('Parts Tab', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Parts Test Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    await page.close()
  })

  async function addPart(page: Page, name: string, opts?: { location?: string; cost?: string; seller?: string }) {
    await page.getByRole('button', { name: '+ Add Part' }).click()
    await page.getByLabel('Part Name').fill(name)
    if (opts?.location) await page.getByLabel('Location').fill(opts.location)
    if (opts?.cost) await page.getByLabel('Cost ($)').fill(opts.cost)
    if (opts?.seller) await page.getByLabel('Seller').fill(opts.seller)
    await page.getByRole('button', { name: 'Add Part', exact: true }).click()
    await expect(page.getByText(name)).toBeVisible()
  }

  test('parts tab shows empty state', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Parts' }).click()
    await expect(page.getByText('No parts yet.')).toBeVisible()
    await expect(page.getByRole('button', { name: '+ Add Part' })).toBeVisible()
  })

  test('create a part with a location', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Parts' }).click()
    await addPart(page, 'GFB DV+', { location: 'Diverter valve', cost: '120.00' })
    await expect(page.getByText('Diverter valve')).toBeVisible()
    await expect(page.getByText('$120.00')).toBeVisible()
    await expect(page.locator('.badge').filter({ hasText: 'purchased' }).first()).toBeVisible()
  })

  test('edit part status to installed', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Parts' }).click()
    await expect(page.getByText('GFB DV+')).toBeVisible()
    await page.locator('.part-card').filter({ hasText: 'GFB DV+' }).getByRole('button', { name: 'Edit' }).click()
    await page.getByLabel('Status').selectOption('installed')
    await page.getByLabel('Installed Date').fill('2026-01-15')
    await page.getByLabel('Installed Odometer').fill('52000')
    await page.getByRole('button', { name: 'Update Part' }).click()
    await expect(page.locator('.badge').filter({ hasText: 'installed' }).first()).toBeVisible()
  })

  test('edit a part location', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Parts' }).click()
    await addPart(page, 'Location Edit Part', { location: 'Front brakes' })
    await page.locator('.part-card').filter({ hasText: 'Location Edit Part' }).getByRole('button', { name: 'Edit' }).click()
    // The form pre-fills the existing location.
    await expect(page.getByLabel('Location')).toHaveValue('Front brakes')
    await page.getByLabel('Location').fill('Rear brakes')
    await page.getByRole('button', { name: 'Update Part' }).click()
    await expect(page.getByText('Rear brakes')).toBeVisible()
  })

  test('installed part can create a linked service inline', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Parts' }).click()
    await page.getByRole('button', { name: '+ Add Part' }).click()
    await page.getByLabel('Part Name').fill('Neuspeed Power Module')
    await page.getByLabel('Seller').fill('ECS Tuning')
    await page.getByLabel('Cost ($)').fill('399.99')
    await page.getByLabel('Status').selectOption('installed')
    await page.getByRole('radio', { name: 'Create new service' }).check()
    await page.getByLabel('Service Date').fill('2026-02-01')
    await page.getByLabel('Description').fill('Power module install')
    await page.getByRole('button', { name: 'Add Part', exact: true }).click()
    await expect(page.getByText('Neuspeed Power Module')).toBeVisible()
    await expect(page.getByText(/via service .*Power module install/)).toBeVisible()
  })

  test('delete a part', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Parts' }).click()
    await addPart(page, 'Doomed Part')
    page.on('dialog', dialog => dialog.accept())
    await page.locator('.part-card').filter({ hasText: 'Doomed Part' }).getByRole('button', { name: 'Delete' }).click()
    await expect(page.getByText('Doomed Part')).not.toBeVisible()
  })

  // TP-19 smoke: parts feed the costs rollup
  test('costs tab shows cost data', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Costs' }).click()
    await expect(page.getByText('Cost of Ownership')).toBeVisible()
    // Wait for loading to finish
    await expect(page.getByText('Loading cost data...')).not.toBeVisible()
    // Parts were added in previous tests; we should see Total Spent
    await expect(page.getByText('Total Spent')).toBeVisible()
  })
})
