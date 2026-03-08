import { test, expect } from '@playwright/test'

// TP-02 & TP-03: Add Vehicle
test.describe('Add Vehicle', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/vehicles/new')
  })

  test('shows VIN step initially', async ({ page }) => {
    await expect(page.getByText('Step 1: Enter VIN')).toBeVisible()
  })

  test('Decode VIN button disabled when input < 17 chars', async ({ page }) => {
    const decodeBtn = page.getByRole('button', { name: 'Decode VIN' })
    await expect(decodeBtn).toBeDisabled()
    await page.getByPlaceholder('Enter 17-character VIN').fill('ABC123')
    await expect(decodeBtn).toBeDisabled()
  })

  test('Skip VIN goes to step 2', async ({ page }) => {
    await page.getByRole('button', { name: 'Skip' }).click()
    await expect(page.getByText('Step 2: Vehicle Details')).toBeVisible()
  })

  test('Back button returns to step 1', async ({ page }) => {
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByRole('button', { name: 'Back' }).click()
    await expect(page.getByText('Step 1: Enter VIN')).toBeVisible()
  })

  test('name field is required by the browser', async ({ page }) => {
    await page.getByRole('button', { name: 'Skip' }).click()
    const nameInput = page.getByLabel('Vehicle Name')
    // HTML5 required attribute prevents submission with empty name
    await expect(nameInput).toHaveAttribute('required', '')
  })

  test('creates vehicle and navigates to detail', async ({ page }) => {
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('E2E Test Car')
    await page.getByLabel('Year').fill('2020')
    await page.getByLabel('Make').fill('Toyota')
    await page.getByRole('textbox', { name: 'Model' }).fill('Corolla')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    // Should navigate to vehicle detail
    await expect(page).toHaveURL(/\/vehicles\/\d+/)
    await expect(page.getByRole('heading', { name: 'E2E Test Car' })).toBeVisible()
  })
})
