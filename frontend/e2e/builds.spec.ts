import { test, expect, type Page } from '@playwright/test'
import { createVehicle, vehicleIdFrom } from './helpers'

// TP-08: Builds tab
test.describe('Builds', () => {
  let vehicleUrl: string
  let vehicleId: number

  test.beforeAll(async ({ browser }) => {
    vehicleUrl = await createVehicle(browser, 'Builds Test Car')
    vehicleId = vehicleIdFrom(vehicleUrl)
  })

  /** Each test seeds its own build (house rule: no prior-test state). */
  async function seedBuild(page: Page, name: string): Promise<number> {
    const res = await page.request.post(`/api/vehicles/${vehicleId}/builds`, {
      data: { name },
    })
    expect(res.ok()).toBe(true)
    return (await res.json()).id
  }

  test('creates a build from the empty state', async ({ page }) => {
    await page.goto(`${vehicleUrl}/builds`)
    // Empty only on the first run through this vehicle; don't depend on it.
    await page.getByRole('button', { name: '+ New build' }).click()
    await page.getByLabel('Name').fill('Turbo swap')
    await page.getByLabel('Description').fill('IS38 conversion')
    await page.getByRole('button', { name: 'Create build' }).click()

    const card = page.locator('.build-card', { hasText: 'Turbo swap' })
    await expect(card).toBeVisible()
    await expect(card.locator('.status-badge')).toHaveText('planned')
    await expect(card.getByText('IS38 conversion')).toBeVisible()
  })

  test('status transition to active surfaces the sidebar chip', async ({ page }) => {
    await seedBuild(page, 'Status Build')
    await page.goto(`${vehicleUrl}/builds`)
    const card = page.locator('.build-card', { hasText: 'Status Build' })
    await card.locator('.build-header').click()
    await card.getByLabel('Status').selectOption('active')
    await expect(card.locator('.status-badge')).toHaveText('active')

    // The sidebar's per-car hint picks up the active build.
    await expect(
      page.getByTestId('sidebar').locator('.entry', { hasText: 'Builds Test Car' }).getByText('build active').first(),
    ).toBeVisible()
  })

  test('progress detail rolls up linked records and deep-links to the Timeline', async ({ page }) => {
    const buildId = await seedBuild(page, 'Progress Build')
    const res = await page.request.post(`/api/vehicles/${vehicleId}/services`, {
      data: {
        service_date: '2026-06-10',
        description: 'Downpipe install',
        total_cost_cents: 45_000,
        build_id: buildId,
      },
    })
    expect(res.ok()).toBe(true)

    await page.goto(`${vehicleUrl}/builds`)
    const card = page.locator('.build-card', { hasText: 'Progress Build' })
    await card.locator('.build-header').click()
    await expect(card.getByText('Total spend')).toBeVisible()
    // Self-paid: total spend AND out-of-pocket both read $450.00.
    await expect(card.locator('.progress-stat', { hasText: 'Total spend' }).getByText('$450.00')).toBeVisible()
    await card.getByRole('button', { name: /1 linked service/ }).click()
    await expect(page).toHaveURL(new RegExp(`${vehicleUrl}/timeline`))
    await expect(page.getByText('Downpipe install')).toBeVisible()
  })
})
