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
    await expect(page.getByText(/No research reports yet/)).toBeVisible()
    await expect(page.getByRole('button', { name: 'Check Recalls' })).toBeVisible()
  })

  test('recall check requires make/model/year', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Research' }).click()
    await page.getByRole('button', { name: 'Check Recalls' }).click()
    // Vehicle was created without make/model/year, so should show error
    await expect(page.getByText(/required for recall lookup/i)).toBeVisible()
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
    // Wait for the check to complete — should show either recall count or "no recalls"
    await expect(
      page.getByText(/recall\(s\) found|No open recalls found/i).first()
    ).toBeVisible({ timeout: 30000 })
  })
})
