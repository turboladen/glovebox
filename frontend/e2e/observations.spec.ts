import { test, expect } from '@playwright/test'

// TP-14: Observations
test.describe('Observations', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Obs Test Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    await page.close()
  })

  test('observations tab shows empty state', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Obs.' }).click()
    await expect(page.getByText('No observations yet.')).toBeVisible()
    await expect(page.getByRole('button', { name: '+ Add Observation' })).toBeVisible()
  })

  test('create an observation', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Obs.' }).click()
    await page.getByRole('button', { name: '+ Add Observation' }).click()
    await page.getByLabel('Title').fill('Rattle on cold start')
    await page.getByLabel('Category').selectOption('noise')
    await page.getByRole('button', { name: 'Save' }).click()
    await expect(page.getByText('Rattle on cold start')).toBeVisible()
  })

  test('resolve an observation', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Obs.' }).click()
    await page.getByRole('button', { name: 'Mark Resolved' }).click()
    await expect(page.getByRole('button', { name: 'Unresolve' })).toBeVisible()
  })

  test('observation appears in history tab', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'History', exact: true }).click()
    await expect(page.getByText('Rattle on cold start')).toBeVisible()
    await expect(page.getByText('Observation', { exact: true })).toBeVisible()
  })
})
