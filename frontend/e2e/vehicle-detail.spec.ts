import { test, expect } from '@playwright/test'
import { createVehicle } from './helpers'

// TP-04: Vehicle Detail shell (intent tabs)
test.describe('Vehicle Detail', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    vehicleUrl = await createVehicle(browser, 'Detail Test Car')
  })

  test('shows vehicle detail layout', async ({ page }) => {
    await page.goto(vehicleUrl)
    await expect(page.getByRole('heading', { name: 'Detail Test Car' })).toBeVisible()
    await expect(page.getByText('← All vehicles')).toBeVisible()
    await expect(page.getByRole('button', { name: 'Update Mileage' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Log Service' })).toBeVisible()
  })

  test('Overview is the default tab and shows the scoped dashboard', async ({ page }) => {
    await page.goto(vehicleUrl)
    await expect(page.getByRole('button', { name: 'Overview' })).toHaveClass(/active/)
    await expect(page.getByTestId('plan-budget-block')).toBeVisible()
  })

  test('intent tabs are URL-driven and deep-linkable', async ({ page }) => {
    await page.goto(vehicleUrl)
    for (const tab of ['Timeline', 'Plan', 'Builds', 'Records', 'Costs']) {
      await page.getByRole('button', { name: tab, exact: true }).click()
      await expect(page).toHaveURL(new RegExp(`${vehicleUrl}/${tab.toLowerCase()}`))
      await expect(page.getByRole('button', { name: tab, exact: true })).toHaveClass(/active/)
    }
    // A direct tab URL lands on that tab.
    await page.goto(`${vehicleUrl}/timeline`)
    await expect(page.getByRole('button', { name: 'Timeline' })).toHaveClass(/active/)
  })

  test('back link returns to the dashboard', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByText('← All vehicles').click()
    await expect(page).toHaveURL('/')
  })
})

// TP-04a-e: Edit Vehicle
test.describe('Edit Vehicle', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    vehicleUrl = await createVehicle(browser, 'Edit Test Car')
  })

  test('toggle edit form', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Edit' }).click()
    await expect(page.getByRole('heading', { name: 'Edit Vehicle' })).toBeVisible()
    await page.getByRole('button', { name: 'Cancel' }).click()
    await expect(page.getByRole('heading', { name: 'Edit Vehicle' })).not.toBeVisible()
  })

  test('edit name updates heading', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Edit' }).click()
    await page.getByLabel('Vehicle Name').fill('Renamed Car')
    await page.getByRole('button', { name: 'Save Changes' }).click()
    await expect(page.getByRole('heading', { name: 'Edit Vehicle' })).not.toBeVisible()
    await expect(page.getByRole('heading', { name: 'Renamed Car' })).toBeVisible()
  })

  test('set vehicle details shows subtitle', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Edit' }).click()
    await page.getByLabel('Year').fill('2020')
    await page.getByLabel('Make').fill('Toyota')
    await page.getByRole('textbox', { name: 'Model' }).fill('Camry')
    await page.getByRole('button', { name: 'Save Changes' }).click()
    await expect(page.locator('.vehicle-subtitle')).toContainText('2020 Toyota Camry')
  })

  test('set sold fields shows badge', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Edit' }).click()
    await page.getByLabel('Sold Date').fill('2025-06-15')
    await page.getByLabel('Sold Price ($)').fill('15000')
    await page.getByLabel('Sold Mileage').fill('85000')
    await page.getByRole('button', { name: 'Save Changes' }).click()
    await expect(page.locator('.sold-badge')).toBeVisible()
    await expect(page.locator('.sold-badge')).toContainText('Sold')
  })

  test('clear sold fields removes badge', async ({ page }) => {
    await page.goto(vehicleUrl)
    // First set sold fields
    await page.getByRole('button', { name: 'Edit' }).click()
    await page.getByLabel('Sold Date').fill('2025-06-15')
    await page.getByRole('button', { name: 'Save Changes' }).click()
    await expect(page.getByRole('heading', { name: 'Edit Vehicle' })).not.toBeVisible()
    await expect(page.locator('.sold-badge')).toBeVisible()
    // Now clear them (edit clears send explicit null)
    await page.getByRole('button', { name: 'Edit' }).click()
    await expect(page.getByRole('heading', { name: 'Edit Vehicle' })).toBeVisible()
    await page.getByLabel('Sold Date').clear()
    await page.getByLabel('Sold Price ($)').clear()
    await page.getByLabel('Sold Mileage').clear()
    await page.getByRole('button', { name: 'Save Changes' }).click()
    await expect(page.getByRole('heading', { name: 'Edit Vehicle' })).not.toBeVisible()
    await expect(page.locator('.sold-badge')).not.toBeVisible()
  })
})

// TP-05: Update Mileage
test.describe('Update Mileage', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    vehicleUrl = await createVehicle(browser, 'Mileage Test Car')
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
    await expect(page.getByLabel('Current Odometer')).not.toBeVisible()
  })

  test('shows exact mileage without est. label after today entry', async ({ page }) => {
    await page.goto(vehicleUrl)
    // Submit mileage within this test so it doesn't depend on prior test state
    await page.getByRole('button', { name: 'Update Mileage' }).click()
    await page.getByLabel('Current Odometer').fill('50000')
    await page.getByRole('button', { name: 'Save' }).click()
    await expect(page.getByLabel('Current Odometer')).not.toBeVisible()
    const mileageUnit = page.locator('.mileage-unit')
    await expect(mileageUnit).toBeVisible()
    await expect(mileageUnit).toHaveText('mi')
    await expect(page.locator('.mileage-date')).toBeVisible()
  })
})
