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
    // The context strip's breadcrumb replaced the old back-link (the
    // sidebar covers "All vehicles"; the strip stays slim).
    await expect(page.getByRole('link', { name: 'Garage' })).toBeVisible()
    // Two everyday verbs at equal weight…
    await expect(page.getByRole('button', { name: 'Update mileage' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Record service' })).toBeVisible()
    // …and the occasional actions behind the ⋯ overflow menu.
    await page.getByRole('button', { name: 'More actions' }).click()
    await expect(page.getByRole('menuitem', { name: 'Edit vehicle…' })).toBeVisible()
    await expect(page.getByRole('menuitem', { name: 'Export history' })).toBeVisible()
    await expect(page.getByRole('menuitem', { name: 'Archive vehicle…' })).toBeVisible()
    // Esc closes the menu.
    await page.keyboard.press('Escape')
    await expect(page.getByRole('menuitem', { name: 'Edit vehicle…' })).not.toBeVisible()
  })

  test('header "Record service" routes to the Timeline with the form open (one verb, one form)', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Record service' }).click()
    // The ?action=record param is CONSUMED once handled (an identical hash
    // push fires no event, so leaving it would kill the second click) —
    // the URL settles on the clean timeline path with the form open.
    await expect(page.getByRole('heading', { name: 'Record service' })).toBeVisible()
    await expect(page).toHaveURL(new RegExp(`${vehicleUrl}/timeline$`))
    await expect(page.getByRole('button', { name: 'Timeline' })).toHaveClass(/active/)
    await expect(page.getByLabel('Description')).toBeVisible()

    // Second click must work too (the dead-affordance regression): close the
    // form, then re-open it from the strip (now the ONLY such button).
    await page.locator('form').getByRole('button', { name: 'Cancel' }).click()
    await expect(page.getByRole('heading', { name: 'Record service' })).not.toBeVisible()
    await page.getByRole('button', { name: 'Record service' }).click()
    await expect(page.getByRole('heading', { name: 'Record service' })).toBeVisible()
  })

  test('one record-service verb everywhere — never two on a screen', async ({ page }) => {
    for (const path of ['', '/timeline', '/costs']) {
      await page.goto(`${vehicleUrl}${path}`)
      await expect(page.getByRole('heading', { name: 'Detail Test Car' })).toBeVisible()
      await expect(page.getByText(/log service/i)).toHaveCount(0)
      // The strip owns the verb; no tab may duplicate it.
      await expect(page.getByRole('button', { name: 'Record service' })).toHaveCount(1)
    }
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

  test('unknown tab and sub-tab URLs fall back instead of a blank pane', async ({ page }) => {
    // Bogus :tab → Overview renders and its tab reads active.
    await page.goto(`${vehicleUrl}/bogus`)
    await expect(page.getByRole('button', { name: 'Overview' })).toHaveClass(/active/)
    await expect(page.getByTestId('plan-budget-block')).toBeVisible()

    // Bogus Plan :sub → Due renders and its chip reads active.
    await page.goto(`${vehicleUrl}/plan/bogus`)
    await expect(page.getByRole('button', { name: 'Due', exact: true })).toHaveClass(/active/)

    // Bogus Records :sub → Parts renders and its chip reads active.
    await page.goto(`${vehicleUrl}/records/bogus`)
    await expect(page.getByRole('button', { name: 'Parts', exact: true })).toHaveClass(/active/)
  })

  test('breadcrumb returns to the dashboard', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('link', { name: 'Garage' }).click()
    await expect(page).toHaveURL('/')
  })
})

// TP-04a-e: Edit Vehicle (opened from the ⋯ overflow menu)
test.describe('Edit Vehicle', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    vehicleUrl = await createVehicle(browser, 'Edit Test Car')
  })

  async function openEditForm(page: import('@playwright/test').Page) {
    await page.getByRole('button', { name: 'More actions' }).click()
    await page.getByRole('menuitem', { name: 'Edit vehicle…' }).click()
  }

  test('toggle edit form', async ({ page }) => {
    await page.goto(vehicleUrl)
    await openEditForm(page)
    await expect(page.getByRole('heading', { name: 'Edit Vehicle' })).toBeVisible()
    await page.getByRole('button', { name: 'Cancel' }).click()
    await expect(page.getByRole('heading', { name: 'Edit Vehicle' })).not.toBeVisible()
  })

  test('edit name updates heading', async ({ page }) => {
    await page.goto(vehicleUrl)
    await openEditForm(page)
    await page.getByLabel('Vehicle Name').fill('Renamed Car')
    await page.getByRole('button', { name: 'Save Changes' }).click()
    await expect(page.getByRole('heading', { name: 'Edit Vehicle' })).not.toBeVisible()
    await expect(page.getByRole('heading', { name: 'Renamed Car' })).toBeVisible()
  })

  test('set vehicle details shows subtitle', async ({ page }) => {
    await page.goto(vehicleUrl)
    await openEditForm(page)
    await page.getByLabel('Year').fill('2020')
    await page.getByLabel('Make').fill('Toyota')
    await page.getByRole('textbox', { name: 'Model' }).fill('Camry')
    await page.getByRole('button', { name: 'Save Changes' }).click()
    await expect(page.locator('.vehicle-subtitle')).toContainText('2020 Toyota Camry')
  })

  test('set sold fields shows badge', async ({ page }) => {
    await page.goto(vehicleUrl)
    await openEditForm(page)
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
    await openEditForm(page)
    await page.getByLabel('Sold Date').fill('2025-06-15')
    await page.getByRole('button', { name: 'Save Changes' }).click()
    await expect(page.getByRole('heading', { name: 'Edit Vehicle' })).not.toBeVisible()
    await expect(page.locator('.sold-badge')).toBeVisible()
    // Now clear them (edit clears send explicit null)
    await openEditForm(page)
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
