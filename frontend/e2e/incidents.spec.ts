import { test, expect } from '@playwright/test'

// TP-14: Incidents (interim tab — unified observations + accidents)
test.describe('Incidents', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Incident Test Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    await page.close()
  })

  test('incidents tab shows empty state', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Incidents' }).click()
    await expect(page.getByText('No incidents yet.')).toBeVisible()
    await expect(page.getByRole('button', { name: '+ Add Incident' })).toBeVisible()
  })

  test('create an incident with a category', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Incidents' }).click()
    await page.getByRole('button', { name: '+ Add Incident' }).click()
    await page.getByLabel('Title').fill('Rattle on cold start')
    await page.getByLabel('Category').selectOption('noise')
    await page.getByRole('button', { name: 'Save' }).click()
    await expect(page.getByText('Rattle on cold start')).toBeVisible()
    await expect(page.getByText('Noise', { exact: true })).toBeVisible()
  })

  test('accident category reveals insurance fields and shows them on the card', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Incidents' }).click()
    await page.getByRole('button', { name: '+ Add Incident' }).click()
    // The accident-only fieldset is hidden for other categories…
    await expect(page.getByText('Accident Details')).not.toBeVisible()
    // …and revealed when category = accident.
    await page.getByLabel('Category').selectOption('accident')
    await expect(page.getByText('Accident Details')).toBeVisible()
    await page.getByLabel('Title').fill('Sideswiped while parked')
    await page.getByLabel('Other Party Name').fill('J. Doe')
    await page.getByLabel('Claim Number').fill('CLM-4521')
    await page.getByRole('button', { name: 'Save' }).click()
    await expect(page.getByText('Sideswiped while parked')).toBeVisible()
    // Expand the card: the insurance details are shown.
    await page.getByText('Sideswiped while parked').click()
    await expect(page.getByText('CLM-4521')).toBeVisible()
    await expect(page.getByText('J. Doe')).toBeVisible()
  })

  test('add a followup to an incident', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Incidents' }).click()
    // Create a dedicated incident for the followup.
    await page.getByRole('button', { name: '+ Add Incident' }).click()
    await page.getByLabel('Title').fill('Followup target incident')
    await page.getByRole('button', { name: 'Save' }).click()
    await expect(page.getByText('Followup target incident')).toBeVisible()

    await page.getByText('Followup target incident').click()
    await expect(page.getByText('No followups yet.')).toBeVisible()
    await page.getByRole('button', { name: '+ Add', exact: true }).click()
    await page.getByLabel('Summary').fill('Called the shop about it')
    await page.getByRole('button', { name: 'Add', exact: true }).click()
    await expect(page.getByText('Called the shop about it')).toBeVisible()
  })

  test('resolve toggle marks and reopens an incident', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Incidents' }).click()
    await page.getByRole('button', { name: '+ Add Incident' }).click()
    await page.getByLabel('Title').fill('Resolve toggle incident')
    await page.getByRole('button', { name: 'Save' }).click()
    await expect(page.getByText('Resolve toggle incident')).toBeVisible()

    await page.getByText('Resolve toggle incident').click()
    await page.getByRole('button', { name: 'Mark Resolved' }).click()
    await expect(page.getByRole('button', { name: 'Reopen' })).toBeVisible()

    await page.getByRole('button', { name: 'Reopen' }).click()
    await expect(page.getByRole('button', { name: 'Mark Resolved' })).toBeVisible()
  })

  test('incident appears in history tab', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Incidents' }).click()
    // Ensure an incident exists
    if (!(await page.getByText('Rattle on cold start').first().isVisible().catch(() => false))) {
      await page.getByRole('button', { name: '+ Add Incident' }).click()
      await page.getByLabel('Title').fill('Rattle on cold start')
      await page.getByRole('button', { name: 'Save' }).click()
      await expect(page.getByText('Rattle on cold start')).toBeVisible()
    }
    await page.getByRole('button', { name: 'History', exact: true }).click()
    await expect(page.getByText('Rattle on cold start').first()).toBeVisible()
    await expect(page.getByText('Incident', { exact: true }).first()).toBeVisible()
  })
})
