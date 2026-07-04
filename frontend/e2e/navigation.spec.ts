import { test, expect } from '@playwright/test'
import { createVehicle, seedOverdueItem, vehicleIdFrom } from './helpers'

// TP-01: Shell — header, sidebar, routing
test.describe('Navigation', () => {
  test('logo links to the dashboard', async ({ page }) => {
    await page.goto('/vehicles/new')
    await page.getByText('Glovebox').click()
    await expect(page).toHaveURL('/')
  })

  test('404 page for unknown routes', async ({ page }) => {
    await page.goto('/nonexistent')
    await expect(page.getByText('404')).toBeVisible()
    await expect(page.getByText('Page not found')).toBeVisible()
    await page.getByText('Back to Garage').click()
    await expect(page).toHaveURL('/')
  })

  test('direct URL to vehicle detail works', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'Nav Test Car')
    await page.goto('/')
    await page.goto(url)
    await expect(page.getByRole('heading', { name: 'Nav Test Car' })).toBeVisible()
  })

  test('sidebar lists vehicles and navigates between them', async ({ browser, page }) => {
    await createVehicle(browser, 'Sidebar Car One')
    await createVehicle(browser, 'Sidebar Car Two')
    await page.goto('/')
    const sidebar = page.getByTestId('sidebar')
    await expect(sidebar.getByText('All vehicles')).toBeVisible()
    await expect(sidebar.getByText('Sidebar Car One')).toBeVisible()

    // Click a car: the scoped view opens with Overview active.
    await sidebar.getByText('Sidebar Car One').click()
    await expect(page).toHaveURL(/\/vehicles\/\d+$/)
    await expect(page.getByRole('heading', { name: 'Sidebar Car One' })).toBeVisible()

    // Switch vehicles directly from the sidebar (in-place reload).
    await sidebar.getByText('Sidebar Car Two').click()
    await expect(page.getByRole('heading', { name: 'Sidebar Car Two' })).toBeVisible()

    // "All vehicles" returns to the garage dashboard.
    await sidebar.getByText('All vehicles').click()
    await expect(page).toHaveURL('/')
  })

  test('sidebar "N due" badge deep-links to the Plan due view', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'Sidebar Due Badge Car')
    const vehicleId = vehicleIdFrom(url)
    await seedOverdueItem(page, vehicleId, 'Badge overdue item')

    await page.goto('/')
    const entry = page
      .getByTestId('sidebar')
      .locator('.entry', { hasText: 'Sidebar Due Badge Car' })
    // The badge is a link to what's due, not just a count…
    await entry.getByRole('button', { name: /due/ }).click()
    await expect(page).toHaveURL(new RegExp(`/vehicles/${vehicleId}/plan/due`))
    await expect(
      page.locator('.reminder-card.overdue', { hasText: 'Badge overdue item' }),
    ).toBeVisible()
  })

  test('sidebar collapses to a slim rail and the state persists', async ({ page }) => {
    await page.goto('/')
    const sidebar = page.getByTestId('sidebar')
    await expect(sidebar).toBeVisible()

    await page.getByRole('button', { name: 'Toggle sidebar' }).click()
    await expect(sidebar).not.toBeVisible()
    await expect(page.getByRole('button', { name: 'Open sidebar' })).toBeVisible()

    // Collapsed state survives a reload (localStorage).
    await page.reload()
    await expect(page.getByTestId('sidebar')).not.toBeVisible()

    // Search stays reachable while collapsed: the rail's ⌕ reopens the
    // sidebar and lands focus in the search box.
    await page.getByRole('button', { name: 'Search' }).click()
    await expect(page.getByTestId('sidebar')).toBeVisible()
    await expect(page.getByRole('searchbox', { name: 'Search' })).toBeFocused()

    // The rail's open button works too.
    await page.getByRole('button', { name: 'Toggle sidebar' }).click()
    await page.getByRole('button', { name: 'Open sidebar' }).click()
    await expect(page.getByTestId('sidebar')).toBeVisible()
  })

  test('sidebar foot links: Shops navigates to the shops page', async ({ page }) => {
    await page.goto('/')
    await page.getByTestId('sidebar').getByRole('link', { name: 'Shops' }).click()
    await expect(page).toHaveURL(/\/shops$/)
    // exact: "Shops" is a substring of the "Manage Shops" heading, which
    // renders data-dependently — without exact this flakes by worker order.
    await expect(page.getByRole('heading', { name: 'Shops', exact: true })).toBeVisible()
  })

  test('global search finds a vehicle and deep-links to it', async ({ browser, page }) => {
    await createVehicle(browser, 'Searchable Xyzzy Wagon')
    await page.goto('/')
    await page.getByRole('searchbox', { name: 'Search' }).fill('Xyzzy')
    await expect(page.getByRole('button', { name: /Searchable Xyzzy Wagon/ })).toBeVisible()
    await page.getByRole('button', { name: /Searchable Xyzzy Wagon/ }).click()
    await expect(page).toHaveURL(/\/vehicles\/\d+$/)
    await expect(page.getByRole('heading', { name: 'Searchable Xyzzy Wagon' })).toBeVisible()
  })

  test('global search deep-links a service hit into the Timeline', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'Search Service Car')
    const vehicleId = parseInt(url.split('/').pop()!, 10)
    const res = await page.request.post(`/api/vehicles/${vehicleId}/services`, {
      data: { service_date: '2026-05-01', description: 'Flux capacitor overhaul' },
    })
    expect(res.ok()).toBe(true)

    await page.goto('/')
    await page.getByRole('searchbox', { name: 'Search' }).fill('flux capacitor')
    await expect(page.getByRole('button', { name: /Flux capacitor overhaul/ })).toBeVisible()
    await page.getByRole('button', { name: /Flux capacitor overhaul/ }).click()
    await expect(page).toHaveURL(new RegExp(`/vehicles/${vehicleId}/timeline`))
    await expect(page.getByText('Flux capacitor overhaul')).toBeVisible()
  })

  test('global search finds a schedule item and lands highlighted on Due', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'Search Schedule Car')
    const vehicleId = vehicleIdFrom(url)
    const itemId = await seedOverdueItem(page, vehicleId, 'Wombat air filter')

    await page.goto('/')
    await page.getByRole('searchbox', { name: 'Search' }).fill('wombat')
    // Schedule-item hits group under MAINTENANCE.
    await expect(page.locator('.hit-group-label', { hasText: 'Maintenance' })).toBeVisible()
    await page.getByRole('button', { name: /Wombat air filter/ }).click()
    await expect(page).toHaveURL(
      new RegExp(`/vehicles/${vehicleId}/plan/due\\?hl=schedule_item:${itemId}`),
    )
    // The Due reminder card scrolls into view and flashes.
    const card = page.locator(`#schedule-item-${itemId}`)
    await expect(card).toBeVisible()
    await expect(card).toHaveClass(/hl-flash/)
  })

  test('schedule-item hit offers a secondary link to its Schedule ⚙ entry', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'Search Config Car')
    const vehicleId = vehicleIdFrom(url)
    const itemId = await seedOverdueItem(page, vehicleId, 'Pangolin coolant flush')

    await page.goto('/')
    await page.getByRole('searchbox', { name: 'Search' }).fill('pangolin')
    await expect(page.getByRole('button', { name: /Pangolin coolant flush/ })).toBeVisible()
    await page.getByRole('button', { name: 'Open in Schedule ⚙' }).click()
    await expect(page).toHaveURL(
      new RegExp(`/vehicles/${vehicleId}/plan/schedule\\?hl=schedule_item:${itemId}`),
    )
    const card = page.locator(`#schedule-item-${itemId}`)
    await expect(card).toBeVisible()
    await expect(card).toHaveClass(/hl-flash/)
  })

  test('global search finds a work item and lands highlighted on To-do', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'Search Todo Car')
    const vehicleId = vehicleIdFrom(url)
    const res = await page.request.post(`/api/vehicles/${vehicleId}/work-items`, {
      data: { title: 'Axolotl downpipe install' },
    })
    expect(res.ok()).toBe(true)
    const itemId = (await res.json()).id as number

    await page.goto('/')
    await page.getByRole('searchbox', { name: 'Search' }).fill('axolotl')
    // Work-item hits group under TO-DO.
    await expect(page.locator('.hit-group-label', { hasText: 'To-do' })).toBeVisible()
    await page.getByRole('button', { name: /Axolotl downpipe install/ }).click()
    await expect(page).toHaveURL(
      new RegExp(`/vehicles/${vehicleId}/plan/todo\\?hl=work_item:${itemId}`),
    )
    const card = page.locator(`#work-item-${itemId}`)
    await expect(card).toBeVisible()
    await expect(card).toHaveClass(/hl-flash/)
  })
})
