import { test, expect } from '@playwright/test'
import { createVehicle, seedOverdueItem, vehicleIdFrom } from './helpers'

// TP-07: Plan tab — Due / To-do / Visits / Schedule ⚙
test.describe('Plan: Due actions', () => {
  let vehicleUrl: string
  let vehicleId: number

  test.beforeAll(async ({ browser }) => {
    vehicleUrl = await createVehicle(browser, 'Plan Due Car')
    vehicleId = vehicleIdFrom(vehicleUrl)
  })

  test('dismiss moves the item to Schedule ⚙; re-enable restores it', async ({ page }) => {
    await seedOverdueItem(page, vehicleId, 'Dismissable item')
    await page.goto(`${vehicleUrl}/plan`)
    const card = page.locator('.reminder-card.overdue', { hasText: 'Dismissable item' })
    await expect(card).toBeVisible()
    await card.getByRole('button', { name: 'Dismiss for this vehicle' }).click()
    await expect(page.locator('.reminder-card.overdue', { hasText: 'Dismissable item' })).toHaveCount(0)

    // The dismissed override lives in the Schedule ⚙ sub-view now.
    await page.getByRole('button', { name: 'Schedule ⚙' }).click()
    const dismissed = page.locator('.item-card.dismissed', { hasText: 'Dismissable item' })
    await expect(dismissed).toBeVisible()
    await expect(dismissed.locator('.overridden-badge')).toBeVisible()

    // Re-enable restores it to the Due view.
    await dismissed.getByRole('button', { name: 'Re-enable' }).click()
    await expect(page.locator('.item-card.dismissed', { hasText: 'Dismissable item' })).toHaveCount(0)
    await page.getByRole('button', { name: 'Due', exact: true }).click()
    await expect(page.locator('.reminder-card.overdue', { hasText: 'Dismissable item' })).toBeVisible()
  })

  test('record service from Due clears the reminder', async ({ page }) => {
    await seedOverdueItem(page, vehicleId, 'Recordable item')
    await page.goto(`${vehicleUrl}/plan`)
    const card = page.locator('.reminder-card.overdue', { hasText: 'Recordable item' })
    await expect(card).toBeVisible()
    await card.getByRole('button', { name: 'Record service…' }).click()
    // Date is prefilled with today and the record links the schedule item
    await card.getByRole('button', { name: 'Save service' }).click()

    const okCard = page.locator('.reminder-card.ok', { hasText: 'Recordable item' })
    await expect(okCard).toBeVisible()
    await expect(okCard.getByRole('button', { name: /1 completion/ })).toBeVisible()
  })

  test('mark done previously backfills a past-dated record', async ({ page }) => {
    await seedOverdueItem(page, vehicleId, 'Backfillable item')
    await page.goto(`${vehicleUrl}/plan`)
    const card = page.locator('.reminder-card.overdue', { hasText: 'Backfillable item' })
    await expect(card).toBeVisible()
    await card.getByRole('button', { name: 'Mark done previously' }).click()

    // Pick a date 3 months back — within the 12-month interval, so it clears
    const past = new Date()
    past.setMonth(past.getMonth() - 3)
    await card.locator('input[type="date"]').fill(past.toISOString().split('T')[0])
    await card.getByRole('button', { name: 'Save past service' }).click()

    await expect(page.locator('.reminder-card.ok', { hasText: 'Backfillable item' })).toBeVisible()
    // The retroactive record is real history — it shows on the Timeline.
    await page.getByRole('button', { name: 'Timeline' }).click()
    await expect(page.locator('.history-list').getByText('Backfillable item').first()).toBeVisible()
  })

  test('plan it puts the reminder on the to-do list with a schedule badge', async ({ page }) => {
    await seedOverdueItem(page, vehicleId, 'Plannable item')
    await page.goto(`${vehicleUrl}/plan`)
    const card = page.locator('.reminder-card.overdue', { hasText: 'Plannable item' })
    await expect(card).toBeVisible()
    await card.getByRole('button', { name: 'Plan it' }).click()
    // The reminder now reads as planned (still overdue until the work happens).
    await expect(card.getByText('planned', { exact: true })).toBeVisible()

    await page.getByRole('button', { name: 'To-do' }).click()
    const row = page.getByTestId('todo-list').locator('.work-card', { hasText: 'Plannable item' })
    await expect(row).toBeVisible()
    await expect(row.getByRole('button', { name: 'schedule' })).toBeVisible()
  })
})

test.describe('Plan: To-do CRUD', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    vehicleUrl = await createVehicle(browser, 'Plan Todo Car')
  })

  test('add, edit, drop, and delete work items', async ({ page }) => {
    await page.goto(`${vehicleUrl}/plan/todo`)
    await expect(page.getByText(/Nothing on the to-do list/)).toBeVisible()

    // Add
    await page.getByRole('button', { name: '+ Add work item' }).click()
    await page.getByLabel('Title').fill('Replace wiper blades')
    await page.getByLabel('Est. cost ($)').fill('35.00')
    await page.getByRole('button', { name: 'Add', exact: true }).click()
    const row = page.locator('.work-card', { hasText: 'Replace wiper blades' })
    await expect(row).toBeVisible()
    await expect(row.getByText('$35.00')).toBeVisible()
    await expect(row.getByText('planned', { exact: true })).toBeVisible()

    // Edit (clearing the estimate sends an explicit null)
    await row.getByRole('button', { name: 'Edit' }).click()
    await page.getByLabel('Est. cost ($)').fill('42.50')
    await page.getByRole('button', { name: 'Update', exact: true }).click()
    await expect(row.getByText('$42.50')).toBeVisible()

    // Drop: disappears from the open list, visible with Show finished.
    await row.getByRole('button', { name: 'Drop' }).click()
    await expect(page.locator('.work-card', { hasText: 'Replace wiper blades' })).toHaveCount(0)
    await page.getByLabel('Show finished').check()
    const dropped = page.locator('.work-card.finished', { hasText: 'Replace wiper blades' })
    await expect(dropped).toBeVisible()
    await expect(dropped.getByText('dropped', { exact: true })).toBeVisible()

    // Delete an open item outright.
    await page.getByRole('button', { name: '+ Add work item' }).click()
    await page.getByLabel('Title').fill('Doomed task')
    await page.getByRole('button', { name: 'Add', exact: true }).click()
    const doomed = page.locator('.work-card', { hasText: 'Doomed task' })
    await expect(doomed).toBeVisible()
    page.on('dialog', (dialog) => dialog.accept())
    await doomed.getByRole('button', { name: 'Delete' }).click()
    await expect(page.locator('.work-card', { hasText: 'Doomed task' })).toHaveCount(0)
  })
})

test.describe('Plan: Visits', () => {
  let vehicleUrl: string
  let vehicleId: number

  test.beforeAll(async ({ browser }) => {
    vehicleUrl = await createVehicle(browser, 'Plan Visits Car')
    vehicleId = vehicleIdFrom(vehicleUrl)
  })

  test('schedule → complete round-trip creates the record and clears the reminder', async ({ page }) => {
    await seedOverdueItem(page, vehicleId, 'Visit water pump', 65_000)
    await page.goto(`${vehicleUrl}/plan`)

    // Plan the overdue item, then group it into a visit.
    const due = page.locator('.reminder-card.overdue', { hasText: 'Visit water pump' })
    await expect(due).toBeVisible()
    await due.getByRole('button', { name: 'Plan it' }).click()
    await expect(due.getByText('planned', { exact: true })).toBeVisible()

    await page.getByRole('button', { name: 'Visits' }).click()
    await page.getByRole('button', { name: '+ Schedule visit' }).click()
    await page.getByLabel('Planned date').fill('2026-07-20')
    await page.getByLabel('Shop name (free text)').fill("Joe's Garage")
    await page.getByRole('checkbox', { name: /Visit water pump/ }).check()
    await page.getByRole('button', { name: 'Schedule visit', exact: true }).click()

    const visitCard = page.locator('.visit-card', { hasText: "Joe's Garage" })
    await expect(visitCard).toBeVisible()
    await expect(visitCard.getByText('planned', { exact: true })).toBeVisible()
    await expect(visitCard.getByText(/Visit water pump/)).toBeVisible()

    // Complete with the actuals — one atomic loop-closer.
    await visitCard.getByRole('button', { name: 'Complete…' }).click()
    await visitCard.getByLabel('Odometer').fill('62000')
    await visitCard.getByLabel('Total ($)').fill('612.50')
    await visitCard.getByRole('button', { name: 'Complete visit' }).click()
    await expect(page.locator('.visit-card', { hasText: "Joe's Garage" })).toHaveCount(0)

    // The reminder cleared…
    await page.getByRole('button', { name: 'Due', exact: true }).click()
    await expect(page.locator('.reminder-card.ok', { hasText: 'Visit water pump' })).toBeVisible()

    // …and the service record exists on the Timeline with the visit's shop.
    await page.getByRole('button', { name: 'Timeline' }).click()
    const svcCard = page.locator('.service-card', { hasText: 'Visit water pump' })
    await expect(svcCard).toBeVisible()
    await expect(svcCard.getByText("at Joe's Garage")).toBeVisible()
    await expect(svcCard.getByText('$612.50')).toBeVisible()
  })

  test('a selected shop is authoritative over the free-text name', async ({ page }) => {
    // Two saved shops to pick between.
    for (const name of ['Authoritative Motors', 'Replacement Garage']) {
      const res = await page.request.post('/api/shops', { data: { name } })
      expect(res.ok()).toBe(true)
    }

    await page.goto(`${vehicleUrl}/plan/visits`)
    await page.getByRole('button', { name: '+ Schedule visit' }).click()
    await page.getByLabel('Shop', { exact: true }).selectOption({ label: 'Authoritative Motors' })
    // With a shop selected, the free-text field is inert (disabled) — the
    // select wins even if it held a stale name.
    await expect(page.getByLabel('Shop name (free text)')).toBeDisabled()
    await page.getByRole('button', { name: 'Schedule visit', exact: true }).click()
    const card = page.locator('.visit-card', { hasText: 'Authoritative Motors' })
    await expect(card).toBeVisible()

    // Editing and selecting a different shop replaces the stored name.
    await card.getByRole('button', { name: 'Edit / attach items' }).click()
    await page.getByLabel('Shop', { exact: true }).selectOption({ label: 'Replacement Garage' })
    await page.getByRole('button', { name: 'Update visit' }).click()
    await expect(page.locator('.visit-card', { hasText: 'Replacement Garage' })).toBeVisible()
    await expect(page.locator('.visit-card', { hasText: 'Authoritative Motors' })).toHaveCount(0)
  })

  test('cancel returns the items to the to-do list', async ({ page }) => {
    await page.goto(`${vehicleUrl}/plan/todo`)
    await page.getByRole('button', { name: '+ Add work item' }).click()
    await page.getByLabel('Title').fill('Canceled visit task')
    await page.getByRole('button', { name: 'Add', exact: true }).click()
    await expect(page.locator('.work-card', { hasText: 'Canceled visit task' })).toBeVisible()

    await page.getByRole('button', { name: 'Visits' }).click()
    await page.getByRole('button', { name: '+ Schedule visit' }).click()
    await page.getByRole('checkbox', { name: /Canceled visit task/ }).check()
    await page.getByRole('button', { name: 'Schedule visit', exact: true }).click()
    const visitCard = page.locator('.visit-card', { hasText: 'Canceled visit task' })
    await expect(visitCard).toBeVisible()

    page.on('dialog', (dialog) => dialog.accept())
    await visitCard.getByRole('button', { name: 'Cancel visit' }).click()
    await expect(page.locator('.visit-card', { hasText: 'Canceled visit task' })).toHaveCount(0)

    // Back on the backlog as planned.
    await page.getByRole('button', { name: 'To-do' }).click()
    const row = page.getByTestId('todo-list').locator('.work-card', { hasText: 'Canceled visit task' })
    await expect(row).toBeVisible()
    await expect(row.getByText('planned', { exact: true })).toBeVisible()
  })
})

// TP-26, TP-27: Plan → Research (moved from Records — research is future
// work, not a record of the past)
test.describe('Plan: Research', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    vehicleUrl = await createVehicle(browser, 'Research Test Car')
  })

  test('research sub-view lives under Plan and shows empty state', async ({ page }) => {
    await page.goto(`${vehicleUrl}/plan/research`)
    await expect(page.getByRole('button', { name: 'Plan', exact: true })).toHaveClass(/active/)
    await expect(page.getByRole('button', { name: 'Research' })).toHaveClass(/active/)
    await expect(page.getByText(/No research reports yet/)).toBeVisible()
    await expect(page.getByRole('button', { name: 'Check Recalls' })).toBeVisible()
  })

  test('legacy records/research deep links redirect to plan/research', async ({ page }) => {
    await page.goto(`${vehicleUrl}/records/research`)
    await expect(page).toHaveURL(new RegExp(`${vehicleUrl}/plan/research`))
    await expect(page.getByRole('button', { name: 'Research' })).toHaveClass(/active/)
    await expect(page.getByRole('button', { name: 'Check Recalls' })).toBeVisible()
  })

  test('recall check requires make/model/year', async ({ page }) => {
    await page.goto(`${vehicleUrl}/plan/research`)
    await page.getByRole('button', { name: 'Check Recalls' }).click()
    // Vehicle was created without make/model/year, so should show error
    await expect(page.getByText(/required for recall lookup/i)).toBeVisible()
  })
})

test.describe('Plan: Research with vehicle details', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    vehicleUrl = await createVehicle(browser, 'Research VW GTI', {
      year: '2017',
      make: 'Volkswagen',
      model: 'Golf GTI',
    })
  })

  test('recall check returns results for known vehicle', async ({ page }) => {
    await page.goto(`${vehicleUrl}/plan/research`)
    await page.getByRole('button', { name: 'Check Recalls' }).click()
    // Live NHTSA request — should show either recall count or "no recalls"
    await expect(
      page.getByText(/recall\(s\) found|No open recalls found/i).first()
    ).toBeVisible({ timeout: 30000 })
  })
})

test.describe('Plan: Schedule ⚙', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    vehicleUrl = await createVehicle(browser, 'Plan Config Car')
  })

  test('add a schedule item with an estimated cost; it feeds Due and the forecast', async ({ page }) => {
    await page.goto(`${vehicleUrl}/plan/schedule`)
    await expect(page.getByText(/No schedule items yet/)).toBeVisible()

    await page.getByRole('button', { name: '+ Add item' }).click()
    await page.getByLabel('Name').fill('Config oil change')
    await page.getByLabel('Interval (months)').fill('6')
    await page.getByLabel('Est. cost ($)').fill('89.99')
    await page.getByRole('button', { name: 'Add', exact: true }).click()

    const item = page.locator('.item-card', { hasText: 'Config oil change' })
    await expect(item).toBeVisible()
    await expect(item.getByText('every 6 mo')).toBeVisible()
    await expect(item.getByText('$89.99/occurrence')).toBeVisible()

    // Edit the estimate.
    await item.getByRole('button', { name: 'Edit' }).click()
    await page.getByLabel('Est. cost ($)').fill('99.99')
    await page.getByRole('button', { name: 'Update', exact: true }).click()
    await expect(item.getByText('$99.99/occurrence')).toBeVisible()

    // The Costs tab surfaces the forecast buckets fed by the estimate.
    await page.getByRole('button', { name: 'Costs' }).click()
    const buckets = page.getByTestId('forecast-buckets')
    await expect(buckets).toBeVisible()
    await expect(buckets.getByText('Projected Maintenance')).toBeVisible()
  })
})
