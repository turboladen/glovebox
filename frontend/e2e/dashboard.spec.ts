import { test, expect } from '@playwright/test'
import { createVehicle, seedOverdueItem, vehicleIdFrom } from './helpers'

// TP-00: Garage-wide dashboard (the login landing) + scoped Overview tab
test.describe('Dashboard', () => {
  test('landing shows the dashboard (or the welcome state on an empty garage)', async ({ page }) => {
    await page.goto('/')
    // The suite shares one DB, so the garage may or may not be empty by
    // the time this runs — both faces of '/' are valid.
    await expect(
      page
        .getByRole('heading', { name: 'Garage' })
        .or(page.getByRole('heading', { name: 'Welcome to Glovebox' })),
    ).toBeVisible()
    // Adding a vehicle is a page-level action on the garage dashboard
    // (round 3) — next to the Garage heading, or the welcome CTA when empty.
    await expect(
      page
        .getByTestId('dashboard')
        .getByRole('link', { name: '+ Add vehicle' })
        .or(page.getByRole('link', { name: 'Add Your First Vehicle' })),
    ).toBeVisible()
    // The sidebar foot carries Shops only — no add-vehicle nav verb.
    await expect(page.getByTestId('sidebar').getByText('+ Add vehicle')).toHaveCount(0)
  })

  test('add-vehicle affordance navigates to /vehicles/new', async ({ page }) => {
    await page.goto('/')
    // Shared-DB suite: either the garage page's "+ Add vehicle" (populated
    // garage) or the welcome CTA (empty garage) — both must click through.
    await page
      .getByTestId('dashboard')
      .getByRole('link', { name: '+ Add vehicle' })
      .or(page.getByRole('link', { name: 'Add Your First Vehicle' }))
      .first()
      .click()
    await expect(page).toHaveURL(/\/vehicles\/new$/)
  })

  test('attention + plan & budget + activity blocks render garage-wide data', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'Dash Attention Car')
    const vehicleId = vehicleIdFrom(url)
    await seedOverdueItem(page, vehicleId, 'Dash overdue flush', 10_000)
    const svc = await page.request.post(`/api/vehicles/${vehicleId}/services`, {
      data: { service_date: '2026-06-01', description: 'Dash activity oil change', total_cost_cents: 4_999 },
    })
    expect(svc.ok()).toBe(true)

    await page.goto('/')
    const attention = page.getByTestId('attention-block')
    await expect(attention).toBeVisible()
    await expect(attention.getByText('Dash Attention Car')).toBeVisible()
    await expect(attention.getByText(/Dash overdue flush/)).toBeVisible()

    // Plan & budget: the overdue occurrence lands in the 12-mo forecast.
    const plan = page.getByTestId('plan-budget-block')
    await expect(plan).toBeVisible()
    await expect(plan.getByText(/12-mo forecast/)).toBeVisible()

    // Activity feed shows the service, vehicle-labeled.
    const activity = page.getByTestId('activity-block')
    await expect(activity.getByText(/Dash activity oil change/)).toBeVisible()
    await expect(activity.getByText('Dash Attention Car')).toBeVisible()
  })

  test('plan-it quick action puts the overdue item on the to-do list', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'Dash PlanIt Car')
    const vehicleId = vehicleIdFrom(url)
    await seedOverdueItem(page, vehicleId, 'PlanIt timing belt', 20_000)

    await page.goto('/')
    const row = page
      .getByTestId('attention-block')
      .locator('.attention-row', { hasText: 'PlanIt timing belt' })
    await expect(row).toBeVisible()
    await row.getByRole('button', { name: 'plan it' }).click()
    // The row flips to a planned marker (no duplicate-creating button).
    await expect(row.getByText('planned', { exact: true })).toBeVisible()

    // The work item is real: it shows on the vehicle's Plan → To-do list.
    await page.goto(`${url}/plan/todo`)
    await expect(page.getByTestId('todo-list').getByText('PlanIt timing belt')).toBeVisible()
  })

  test('planned chip links to the work item (highlighted) and ✕ un-plans it', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'Dash Hypermedia Car')
    const vehicleId = vehicleIdFrom(url)
    await seedOverdueItem(page, vehicleId, 'Hypermedia diff fluid')

    await page.goto('/')
    const row = page
      .getByTestId('attention-block')
      .locator('.attention-row', { hasText: 'Hypermedia diff fluid' })
    await row.getByRole('button', { name: 'plan it' }).click()

    // The "planned" state display IS a link to the created work item…
    const chip = row.getByRole('link', { name: 'planned' })
    await expect(chip).toBeVisible()
    await chip.click()
    await expect(page).toHaveURL(new RegExp(`/vehicles/${vehicleId}/plan/todo\\?hl=work_item:\\d+`))

    // …whose row the To-do view scrolls to and flashes.
    const item = page.getByTestId('todo-list').locator('.work-card', { hasText: 'Hypermedia diff fluid' })
    await expect(item).toBeVisible()
    await expect(item).toHaveClass(/hl-flash/)

    // Un-plan from the dashboard chip: confirm-free, reverts to "plan it".
    await page.goto('/')
    await row.getByRole('button', { name: 'Un-plan' }).click()
    await expect(row.getByRole('button', { name: 'plan it' })).toBeVisible()
    await expect(row.getByRole('link', { name: 'planned' })).toHaveCount(0)

    // The work item is really gone from the backlog.
    await page.goto(`${url}/plan/todo`)
    await expect(page.getByText(/Nothing on the to-do list/)).toBeVisible()
  })

  test('attention row deep-links into the vehicle Plan tab', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'Dash DeepLink Car')
    const vehicleId = vehicleIdFrom(url)
    await seedOverdueItem(page, vehicleId, 'DeepLink coolant flush')

    await page.goto('/')
    await page
      .getByTestId('attention-block')
      .getByRole('link', { name: /DeepLink coolant flush/ })
      .click()
    await expect(page).toHaveURL(new RegExp(`/vehicles/${vehicleId}/plan/due`))
    await expect(page.locator('.reminder-card.overdue', { hasText: 'DeepLink coolant flush' })).toBeVisible()
  })

  test('garage Recent activity orders by date-added and annotates · added', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'x01g Added-Order Car')
    const vehicleId = vehicleIdFrom(url)

    // Control row: a same-day service (event date == added date) — must show
    // NO "· added" note. created_at is a UTC datetime the frontend renders in
    // local tz, while service_date (date-only) renders at local midnight, so
    // set service_date to created_at's LOCAL calendar day (not "today" — that
    // flips wrong in the evening for tz's behind UTC).
    const controlRes = await page.request.post(`/api/vehicles/${vehicleId}/services`, {
      data: { service_date: '2026-01-01', description: 'x01g same-day control' },
    })
    expect(controlRes.ok()).toBe(true)
    const control = await controlRes.json()
    const added = new Date(`${control.created_at.replace(' ', 'T')}Z`)
    const localDay = `${added.getFullYear()}-${String(added.getMonth() + 1).padStart(2, '0')}-${String(added.getDate()).padStart(2, '0')}`
    const fixRes = await page.request.put(`/api/vehicles/${vehicleId}/services/${control.id}`, {
      data: { service_date: localDay },
    })
    expect(fixRes.ok()).toBe(true)

    // Old-event row: created LAST, so its created_at is the newest in the DB.
    // Its event date is years old, so under a regression to event-ordering it
    // would sink BELOW the same-day control — this row-order assertion flips
    // red on exactly that revert.
    const oldRes = await page.request.post(`/api/vehicles/${vehicleId}/services`, {
      data: { service_date: '2020-03-15', description: 'x01g old-event brakes' },
    })
    expect(oldRes.ok()).toBe(true)

    await page.goto('/')
    const block = page.getByTestId('activity-block')
    const oldRow = block.locator('.row', { hasText: 'x01g old-event brakes' })
    const controlRow = block.locator('.row', { hasText: 'x01g same-day control' })
    await expect(oldRow).toBeVisible()
    await expect(controlRow).toBeVisible()

    // Added-order: the old-event row (added last) sits ABOVE the same-day
    // control (added first). Compare vertical position — robust to other
    // specs' rows in the shared-DB, multi-worker feed (no .first()/index).
    const oldBox = await oldRow.boundingBox()
    const controlBox = await controlRow.boundingBox()
    expect(oldBox).not.toBeNull()
    expect(controlBox).not.toBeNull()
    expect(oldBox!.y).toBeLessThan(controlBox!.y)

    // The old-event row shows its (old) event date plus the muted "· added" note.
    // Assert the event YEAR (locale-invariant) rather than the fully formatted
    // string — formatDate uses toLocaleDateString and the config sets no locale.
    await expect(oldRow.locator('.activity-date')).toContainText('2020')
    await expect(oldRow.locator('.activity-added')).toHaveText(/· added/)
    // The same-day control shows no "· added" note.
    await expect(controlRow.locator('.activity-added')).toHaveCount(0)
  })

  test('per-car Overview is the same dashboard scoped to the vehicle', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'Dash Scoped Car')
    const vehicleId = vehicleIdFrom(url)
    await seedOverdueItem(page, vehicleId, 'Scoped overdue item')

    await page.goto(url)
    // Overview is the default tab.
    await expect(page.getByRole('button', { name: 'Overview' })).toHaveClass(/active/)
    const attention = page.getByTestId('attention-block')
    await expect(attention.getByText(/Scoped overdue item/)).toBeVisible()
    // Scoped: rows are NOT vehicle-labeled, and other cars' items don't appear.
    await expect(attention.getByText('Dash Attention Car')).not.toBeVisible()
    await expect(page.getByTestId('plan-budget-block')).toBeVisible()
  })
})
