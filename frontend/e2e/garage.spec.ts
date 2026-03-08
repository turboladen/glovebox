import { test, expect } from '@playwright/test'

// TP-01: Garage (Home Page)
test.describe('Garage', () => {
  test('shows heading and Add Car button', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByRole('heading', { name: 'Garage' })).toBeVisible()
    await expect(page.getByRole('link', { name: '+ Add Car' })).toBeVisible()
  })

  test('shows empty state when no vehicles', async ({ page }) => {
    await page.goto('/')
    // If DB is empty, we expect the empty message
    const empty = page.getByText('No vehicles yet.')
    const cards = page.locator('.vehicle-card')
    // One of these should be true
    const hasEmpty = await empty.isVisible().catch(() => false)
    const hasCards = (await cards.count()) > 0
    expect(hasEmpty || hasCards).toBe(true)
  })

  test('Add Car link navigates to /vehicles/new', async ({ page }) => {
    await page.goto('/')
    await page.getByRole('link', { name: /Add/ }).first().click()
    await expect(page).toHaveURL(/\/vehicles\/new/)
  })
})
