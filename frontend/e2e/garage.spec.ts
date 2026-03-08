import { test, expect } from '@playwright/test'

// TP-01: Garage (Home Page)
test.describe('Garage', () => {
  test('shows heading and Add Car button', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByRole('heading', { name: 'Garage' })).toBeVisible()
    await expect(page.getByRole('link', { name: '+ Add Car' })).toBeVisible()
  })

  test('shows vehicles or empty state after loading', async ({ page }) => {
    await page.goto('/')
    // Wait for loading to finish
    await expect(page.getByText('Loading...')).not.toBeVisible({ timeout: 10_000 })
    // Now one of these should be true
    const hasEmpty = await page.getByText('No vehicles yet.').isVisible().catch(() => false)
    const hasCards = (await page.locator('.vehicle-card').count()) > 0
    expect(hasEmpty || hasCards).toBe(true)
  })

  test('Add Car link navigates to /vehicles/new', async ({ page }) => {
    await page.goto('/')
    await page.getByRole('link', { name: /Add/ }).first().click()
    await expect(page).toHaveURL(/\/vehicles\/new/)
  })
})
