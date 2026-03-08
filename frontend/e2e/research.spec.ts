import { test, expect } from '@playwright/test'

// TP-26, TP-27: Research & Recalls
test.describe('Research', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Research Test Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    await page.close()
  })

  test('research tab shows empty state', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Research' }).click()
    await expect(page.getByText('No research reports yet.')).toBeVisible()
    await expect(page.getByRole('button', { name: 'Check Recalls' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Run Full Check' })).toBeVisible()
  })

  test('recall check requires make/model/year', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Research' }).click()
    await page.getByRole('button', { name: 'Check Recalls' }).click()
    // Vehicle was created without make/model/year, so should show error
    await expect(page.getByText(/required for recall lookup/i)).toBeVisible()
  })

  test('generate research report', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Research' }).click()
    await page.getByRole('button', { name: 'Run Full Check' }).click()
    await expect(page.getByRole('button', { name: 'Generating...' })).toBeVisible()
    // Wait for report to load (may fail NHTSA check due to missing vehicle info, but report still created)
    await expect(page.getByText(/Found \d+ items|No findings/).first()).toBeVisible({ timeout: 15000 })
  })

  test('report appears in reports list', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Research' }).click()
    // Wait for reports to load
    await expect(page.locator('.report-type', { hasText: 'Full Check' })).toBeVisible({ timeout: 5000 })
  })
})

// Test with a vehicle that has make/model/year for recall lookups
test.describe('Research with vehicle details', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Research VW GTI')
    await page.getByLabel('Year').fill('2017')
    await page.getByRole('textbox', { name: 'Make' }).fill('Volkswagen')
    await page.getByRole('textbox', { name: /^Model$/ }).fill('Golf GTI')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    await page.close()
  })

  test('recall check returns results for known vehicle', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Research' }).click()
    await page.getByRole('button', { name: 'Check Recalls' }).click()
    // Should either show recalls or "No open recalls found"
    await expect(
      page.getByText(/recall|No open recalls found/i)
    ).toBeVisible({ timeout: 15000 })
  })
})
