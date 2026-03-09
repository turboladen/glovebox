import { test, expect } from '@playwright/test'

// TP-04, TP-05, TP-06: Vehicle Detail, Mileage, Service
test.describe('Vehicle Detail', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    // Create a vehicle to test with
    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Detail Test Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    await page.close()
  })

  test('shows vehicle detail layout', async ({ page }) => {
    await page.goto(vehicleUrl)
    await expect(page.getByRole('heading', { name: 'Detail Test Car' })).toBeVisible()
    await expect(page.getByText('← Garage')).toBeVisible()
    await expect(page.getByRole('button', { name: 'Update Mileage' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Log Service' })).toBeVisible()
  })

  test('schedule tab is active by default', async ({ page }) => {
    await page.goto(vehicleUrl)
    const scheduleTab = page.getByRole('button', { name: 'Schedule' })
    await expect(scheduleTab).toHaveClass(/active/)
  })

  test('can switch to history tab', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'History', exact: true }).click()
    const historyTab = page.getByRole('button', { name: 'History', exact: true })
    await expect(historyTab).toHaveClass(/active/)
  })

  test('back link returns to garage', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByText('← Garage').click()
    await expect(page).toHaveURL('/')
  })
})

// TP-05: Update Mileage
test.describe('Update Mileage', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Mileage Test Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    await page.close()
  })

  test('toggle mileage form', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Update Mileage' }).click()
    await expect(page.getByLabel('Current Odometer')).toBeVisible()
    await page.getByRole('button', { name: 'Cancel' }).click()
    await expect(page.getByLabel('Current Odometer')).not.toBeVisible()
  })

  test('submit valid mileage', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Update Mileage' }).click()
    await page.getByLabel('Current Odometer').fill('45000')
    await page.getByRole('button', { name: 'Save' }).click()
    // Form should close after save
    await expect(page.getByLabel('Current Odometer')).not.toBeVisible()
  })

  test('shows exact mileage without est. label after today entry', async ({ page }) => {
    await page.goto(vehicleUrl)
    // Submit mileage within this test so it doesn't depend on prior test state
    await page.getByRole('button', { name: 'Update Mileage' }).click()
    await page.getByLabel('Current Odometer').fill('50000')
    await page.getByRole('button', { name: 'Save' }).click()
    await expect(page.getByLabel('Current Odometer')).not.toBeVisible()
    // After submitting mileage today, should show "mi" without "est."
    const mileageUnit = page.locator('.mileage-unit')
    await expect(mileageUnit).toBeVisible()
    await expect(mileageUnit).toHaveText('mi')
    // Should not show "as of" date for exact mileage
    await expect(page.locator('.mileage-date')).not.toBeVisible()
  })
})

// TP-06: Log Service
test.describe('Log Service', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Service Test Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    await page.close()
  })

  test('toggle service form', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Log Service' }).click()
    await expect(page.getByRole('heading', { name: 'Log Service' })).toBeVisible()
    await page.getByRole('button', { name: 'Cancel' }).click()
  })

  test('submit service record', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Log Service' }).click()
    await page.getByLabel('Odometer').fill('45200')
    await page.getByLabel('Description').fill('Oil Change')
    await page.getByLabel('Total Cost ($)').fill('49.99')
    await page.getByLabel('Shop').fill('Quick Lube')
    await page.getByRole('button', { name: 'Save Service' }).click()
    // Form should close
    await expect(page.getByLabel('Description')).not.toBeVisible()
    // Check history tab
    await page.getByRole('button', { name: 'History', exact: true }).click()
    await expect(page.getByText('Oil Change')).toBeVisible()
    await expect(page.getByText('$49.99')).toBeVisible()
  })
})
