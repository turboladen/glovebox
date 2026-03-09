import { test, expect, type Page } from '@playwright/test'

// TP-18: Parts Tab
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

  async function createSlot(page: Page, name: string, category: string) {
    await page.getByRole('button', { name: '+ Add Slot' }).click()
    await page.getByLabel('Name').fill(name)
    await page.getByLabel('Category').selectOption(category)
    await page.getByRole('button', { name: 'Create Slot' }).click()
    await expect(page.getByText(name)).toBeVisible()
  }

  async function addPartToSlot(page: Page, partName: string, cost: string) {
    await page.locator('.slot-card').first().getByRole('button', { name: '+ Part' }).click()
    await page.getByLabel('Part Name').fill(partName)
    await page.getByLabel('Cost ($)').fill(cost)
    await page.getByRole('button', { name: 'Add Part', exact: true }).click()
    await expect(page.getByRole('button', { name: /Show history/ })).toBeVisible()
  }

  test('parts tab shows empty state', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Parts' }).click()
    await expect(page.getByText('No parts or slots yet.')).toBeVisible()
    await expect(page.getByRole('button', { name: '+ Add Slot' })).toBeVisible()
  })

  test('create a part slot', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Parts' }).click()
    // Remove any existing data dependency — create fresh
    await createSlot(page, 'Diverter Valve', 'engine')
    await expect(page.getByText('No part installed')).toBeVisible()
  })

  test('add a part to a slot', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Parts' }).click()
    await expect(page.getByText('Diverter Valve')).toBeVisible()
    await addPartToSlot(page, 'GFB DV+', '120.00')
  })

  test('edit part status to installed', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Parts' }).click()
    await expect(page.getByText('Diverter Valve')).toBeVisible()
    await page.getByRole('button', { name: /Show history/ }).click()
    await page.locator('.part-history').getByRole('button', { name: 'Edit' }).click()
    await page.getByLabel('Status').selectOption('installed')
    await page.getByLabel('Installed Date').fill('2026-01-15')
    await page.getByLabel('Installed Odometer').fill('52000')
    await page.getByRole('button', { name: 'Update Part' }).click()
    // The installed part shows in the main view and history — use first()
    await expect(page.locator('.badge').filter({ hasText: 'installed' }).first()).toBeVisible()
  })

  test('slot dropdown defaults to None from header button', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Parts' }).click()
    await createSlot(page, 'Dropdown Test Slot', 'engine')
    await page.locator('.header-actions').getByRole('button', { name: '+ Add Part' }).click()
    const slotSelect = page.getByLabel('Slot')
    await expect(slotSelect).toBeVisible()
    // Header "+ Add Part" should default to "None (unslotted)"
    await expect(slotSelect.locator('option:checked')).toHaveText('None (unslotted)')
  })

  test('slot dropdown pre-selects slot from slot button', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Parts' }).click()
    await createSlot(page, 'Preselect Test Slot', 'brakes')
    await page.locator('.slot-card').filter({ hasText: 'Preselect Test Slot' }).getByRole('button', { name: '+ Part' }).click()
    const slotSelect = page.getByLabel('Slot')
    await expect(slotSelect).toBeVisible()
    // Should be pre-selected to the clicked slot
    await expect(slotSelect.locator('option:checked')).toHaveText(/Preselect Test Slot/)
  })

  test('add an unslotted part', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Parts' }).click()
    await expect(page.getByText('Diverter Valve')).toBeVisible()
    await page.locator('.header-actions').getByRole('button', { name: '+ Add Part' }).click()
    await page.getByLabel('Part Name').fill('Neuspeed Power Module')
    await page.getByLabel('Seller').fill('ECS Tuning')
    await page.getByLabel('Cost ($)').fill('399.99')
    await page.getByRole('button', { name: 'Add Part', exact: true }).click()
    await expect(page.getByText('Neuspeed Power Module')).toBeVisible()
    await expect(page.getByText('Unslotted Parts')).toBeVisible()
  })

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
