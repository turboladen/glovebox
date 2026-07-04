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
    await expect(
      page
        .getByTestId('sidebar')
        .getByText('+ Add vehicle')
        .or(page.getByRole('link', { name: 'Add Your First Vehicle' })),
    ).toBeVisible()
  })

  test('add-vehicle affordance navigates to /vehicles/new', async ({ page }) => {
    await page.goto('/')
    // Shared-DB suite: either the sidebar's "+ Add vehicle" (populated
    // garage) or the welcome CTA (empty garage) — both must click through.
    await page
      .getByTestId('sidebar')
      .getByText('+ Add vehicle')
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
