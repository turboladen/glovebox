import { type Browser, type Page, expect } from '@playwright/test'

/** Create a vehicle through the real Add Vehicle flow; returns its detail
 *  pathname (e.g. /vehicles/7). */
export async function createVehicle(
  browser: Browser,
  name: string,
  opts?: { year?: string; make?: string; model?: string },
): Promise<string> {
  const page = await browser.newPage()
  await page.goto('/vehicles/new')
  await page.getByRole('button', { name: 'Skip' }).click()
  await page.getByLabel('Vehicle Name').fill(name)
  if (opts?.year) await page.getByLabel('Year').fill(opts.year)
  if (opts?.make) await page.getByRole('textbox', { name: 'Make' }).fill(opts.make)
  if (opts?.model) await page.getByRole('textbox', { name: /^Model$/ }).fill(opts.model)
  await page.getByRole('button', { name: 'Create Vehicle' }).click()
  await page.waitForURL(/\/vehicles\/\d+/)
  const path = new URL(page.url()).pathname
  await page.close()
  return path
}

export function vehicleIdFrom(vehicleUrl: string): number {
  return parseInt(vehicleUrl.split('/').pop()!, 10)
}

/** Seed an overdue 12-month schedule item: backdate the purchase and add a
 *  vehicle-level item via the API (the UI equivalent lives in Plan →
 *  Schedule ⚙, exercised by its own test). Returns the item id. */
export async function seedOverdueItem(page: Page, vehicleId: number, name: string, estCostCents?: number): Promise<number> {
  const res = await page.request.put(`/api/vehicles/${vehicleId}`, {
    data: { purchase_date: '2020-01-01' },
  })
  expect(res.ok()).toBe(true)
  const item = await page.request.post('/api/schedules', {
    data: { vehicle_id: vehicleId, name, interval_months: 12, est_cost_cents: estCostCents },
  })
  expect(item.ok()).toBe(true)
  return (await item.json()).id as number
}
