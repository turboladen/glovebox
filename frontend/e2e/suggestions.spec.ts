import { test, expect } from '@playwright/test'

// Helper: delete all AI providers to simulate "not configured" state
async function clearProviders(page: import('@playwright/test').Page) {
  const res = await page.request.get('/api/ai/providers')
  const providers = await res.json()
  for (const p of providers) {
    await page.request.delete(`/api/ai/providers/${p.id}`)
  }
}

// TP-25: Proactive Suggestions
test.describe('AI Suggestions', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Suggestions Test Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    await page.close()
  })

  test('schedule tab loads without suggestions when AI not configured', async ({ page }) => {
    await clearProviders(page)
    await page.goto(vehicleUrl)
    // Schedule tab is active by default
    // SuggestionsCard should not render when AI is not configured
    await expect(page.getByText('AI Suggestions')).not.toBeVisible()
  })

  test('schedule tab loads without errors', async ({ page }) => {
    await page.goto(vehicleUrl)
    // Schedule tab is the default — verify it renders (mileage bar is present)
    await expect(page.locator('.mileage-readout')).toBeVisible()
  })
})
