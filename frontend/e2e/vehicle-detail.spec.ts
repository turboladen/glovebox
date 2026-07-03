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

// TP-04a-e: Edit Vehicle
test.describe('Edit Vehicle', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Edit Test Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    await page.close()
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
    // Now clear them
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
    // Date should still be visible (shows when reading was taken)
    await expect(page.locator('.mileage-date')).toBeVisible()
  })
})

// TP-07a: Schedule overdue resolvability (dismiss / record / mark done previously)
test.describe('Schedule Actions', () => {
  let vehicleUrl: string
  let vehicleId: number

  test.beforeAll(async ({ browser }) => {
    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Schedule Actions Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    vehicleId = parseInt(vehicleUrl.split('/').pop()!, 10)
    // Old purchase date makes every 12-month item overdue from the start
    await page.request.put(`/api/vehicles/${vehicleId}`, {
      data: { purchase_date: '2020-01-01' },
    })
    for (const name of ['Dismissable item', 'Recordable item', 'Backfillable item']) {
      await page.request.post('/api/schedules', {
        data: { vehicle_id: vehicleId, name, interval_months: 12 },
      })
    }
    await page.close()
  })

  test('dismiss greys the item and reminders drop it; re-enable restores', async ({ page }) => {
    await page.goto(vehicleUrl)
    const card = page.locator('.reminder-card.overdue', { hasText: 'Dismissable item' })
    await expect(card).toBeVisible()
    await card.getByRole('button', { name: 'Dismiss for this vehicle' }).click()

    // Moves out of the reminder groups into the greyed Dismissed section
    const dismissed = page.locator('.reminder-card.dismissed', { hasText: 'Dismissable item' })
    await expect(dismissed).toBeVisible()
    await expect(dismissed.locator('.overridden-badge')).toBeVisible()
    await expect(page.locator('.reminder-card.overdue', { hasText: 'Dismissable item' })).toHaveCount(0)

    // Re-enable restores it to the overdue group
    await dismissed.getByRole('button', { name: 'Re-enable' }).click()
    await expect(page.locator('.reminder-card.overdue', { hasText: 'Dismissable item' })).toBeVisible()
    await expect(page.locator('.reminder-card.dismissed', { hasText: 'Dismissable item' })).toHaveCount(0)
  })

  test('record service from the schedule clears the reminder', async ({ page }) => {
    await page.goto(vehicleUrl)
    const card = page.locator('.reminder-card.overdue', { hasText: 'Recordable item' })
    await expect(card).toBeVisible()
    await card.getByRole('button', { name: 'Record service…' }).click()
    // Date is prefilled with today and the record links the schedule item
    await card.getByRole('button', { name: 'Save service' }).click()

    const okCard = page.locator('.reminder-card.ok', { hasText: 'Recordable item' })
    await expect(okCard).toBeVisible()
    await expect(okCard.getByRole('button', { name: /1 completion/ })).toBeVisible()
  })

  test('mark done previously backfills a past-dated record and clears the reminder', async ({ page }) => {
    await page.goto(vehicleUrl)
    const card = page.locator('.reminder-card.overdue', { hasText: 'Backfillable item' })
    await expect(card).toBeVisible()
    await card.getByRole('button', { name: 'Mark done previously' }).click()

    // Pick a date 3 months back — within the 12-month interval, so it clears
    const past = new Date()
    past.setMonth(past.getMonth() - 3)
    await card.locator('input[type="date"]').fill(past.toISOString().split('T')[0])
    await card.getByRole('button', { name: 'Save past service' }).click()

    const okCard = page.locator('.reminder-card.ok', { hasText: 'Backfillable item' })
    await expect(okCard).toBeVisible()
    // The retroactive record is real history
    await page.getByRole('button', { name: 'History', exact: true }).click()
    await expect(page.getByText('Backfillable item').first()).toBeVisible()
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

  test('insurance-paid service shows costs split', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Log Service' }).click()
    await page.getByLabel('Description').fill('Collision repair')
    await page.getByLabel('Total Cost ($)').fill('150.00')
    // Payer note field only appears once a non-self payer is chosen
    await expect(page.getByLabel('Payer Note')).not.toBeVisible()
    await page.getByLabel('Paid By').selectOption('insurance')
    await page.getByLabel('Payer Note').fill('Progressive claim #12345')
    await page.getByRole('button', { name: 'Save Service' }).click()
    await expect(page.getByLabel('Description')).not.toBeVisible()

    // Costs tab splits out-of-pocket vs covered
    await page.getByRole('button', { name: 'Costs' }).click()
    const covered = page.locator('.summary-card', { hasText: 'Covered by Others' })
    await expect(covered).toBeVisible()
    await expect(covered).toContainText('$150.00')
    await expect(page.locator('.summary-card', { hasText: 'Out of Pocket' })).toBeVisible()
  })
})
