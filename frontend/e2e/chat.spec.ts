import { test, expect } from '@playwright/test'

// Helper: delete all AI providers to simulate "not configured" state
async function clearProviders(page: import('@playwright/test').Page) {
  const res = await page.request.get('/api/ai/providers')
  const providers = await res.json()
  for (const p of providers) {
    await page.request.delete(`/api/ai/providers/${p.id}`)
  }
}

// TP-23: AI Chat
test.describe('AI Chat Tab', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Chat Test Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    await page.close()
  })

  test('AI tab is visible in vehicle detail', async ({ page }) => {
    await page.goto(vehicleUrl)
    await expect(page.getByRole('button', { name: 'AI' })).toBeVisible()
  })

  test('shows not-configured message when AI is not set up', async ({ page }) => {
    await clearProviders(page)
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'AI' }).click()
    await expect(page.getByText('AI is not configured')).toBeVisible()
    await expect(page.getByText('Set an AI provider in Settings')).toBeVisible()
  })

  test('chat input is not shown when AI is not configured', async ({ page }) => {
    await clearProviders(page)
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'AI' }).click()
    await expect(page.getByText('AI is not configured')).toBeVisible()
    // Textarea and Send button should not be present
    await expect(page.locator('.chat-input')).not.toBeVisible()
  })
})
