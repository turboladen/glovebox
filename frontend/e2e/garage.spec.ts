import { test, expect } from '@playwright/test'

// TP-01: Garage (Home Page)
test.describe('Garage', () => {
  test('shows heading and Add Car button', async ({ page }) => {
    await page.goto('/')
    // Wait for the garage to finish loading (don't race the loading skeleton).
    await expect(page.getByText('Loading garage...')).not.toBeVisible({ timeout: 10_000 })
    // The home page renders either the populated garage header or, when empty, the
    // "Welcome to Glovebox" first-run state — both are valid and both expose an
    // add-vehicle action. (CI starts with an empty DB, so don't assume vehicles exist.)
    await expect(
      page
        .getByRole('heading', { name: 'Garage' })
        .or(page.getByRole('heading', { name: 'Welcome to Glovebox' })),
    ).toBeVisible()
    await expect(
      page
        .getByRole('link', { name: '+ Add Car' })
        .or(page.getByRole('link', { name: 'Add Your First Vehicle' })),
    ).toBeVisible()
  })

  test('shows vehicles or empty state after loading', async ({ page }) => {
    await page.goto('/')
    // Wait for loading to finish
    await expect(page.getByText('Loading garage...')).not.toBeVisible({ timeout: 10_000 })
    // Now one of these should be true
    const hasEmpty = await page.getByText('Your garage is empty').isVisible().catch(() => false)
    const hasCards = (await page.locator('.vehicle-card').count()) > 0
    expect(hasEmpty || hasCards).toBe(true)
  })

  test('Add Car link navigates to /vehicles/new', async ({ page }) => {
    await page.goto('/')
    await page.getByRole('link', { name: /Add/ }).first().click()
    await expect(page).toHaveURL(/\/vehicles\/new/)
  })
})
