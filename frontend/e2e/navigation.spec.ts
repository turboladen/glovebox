import { test, expect } from '@playwright/test'

// TP-10: Navigation & Routing
test.describe('Navigation', () => {
  test('logo links to garage', async ({ page }) => {
    await page.goto('/vehicles/new')
    await page.getByText('Glovebox').click()
    await expect(page).toHaveURL('/')
  })

  test('404 page for unknown routes', async ({ page }) => {
    await page.goto('/nonexistent')
    await expect(page.getByText('404')).toBeVisible()
    await expect(page.getByText('Page not found')).toBeVisible()
    await page.getByText('Back to Garage').click()
    await expect(page).toHaveURL('/')
  })

  test('direct URL to vehicle detail works', async ({ page }) => {
    // Create a vehicle first
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Nav Test Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    const url = page.url()
    // Navigate away and back via direct URL
    await page.goto('/')
    await page.goto(url)
    await expect(page.getByRole('heading', { name: 'Nav Test Car' })).toBeVisible()
  })
})
