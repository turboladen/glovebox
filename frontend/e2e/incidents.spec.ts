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
    // Resolving now offers an optional service link; decline it.
    await page.getByLabel('Link to a service').selectOption({ label: 'Resolve without service' })
    await expect(page.getByRole('button', { name: 'Reopen' })).toBeVisible()

    await page.getByRole('button', { name: 'Reopen' }).click()
    await expect(page.getByRole('button', { name: 'Mark Resolved' })).toBeVisible()
  })

  test('resolving with a service links it and surfaces chips', async ({ page }) => {
    await page.goto(vehicleUrl)
    // Create a service record to link against.
    await page.getByRole('button', { name: 'Log Service' }).click()
    await page.getByLabel('Description').fill('Brake pad replacement')
    await page.getByRole('button', { name: 'Save Service' }).click()
    await expect(page.getByLabel('Description')).not.toBeVisible()

    await page.getByRole('button', { name: 'Incidents' }).click()
    await page.getByRole('button', { name: '+ Add Incident' }).click()
    await page.getByLabel('Title').fill('Grinding when braking')
    await page.getByRole('button', { name: 'Save' }).click()
    await expect(page.getByText('Grinding when braking')).toBeVisible()

    // Resolve via the picker, choosing the service.
    await page.getByText('Grinding when braking').click()
    await page.getByRole('button', { name: 'Mark Resolved' }).click()
    const picker = page.getByLabel('Link to a service')
    const optionValue = await picker
      .locator('option', { hasText: 'Brake pad replacement' })
      .getAttribute('value')
    await picker.selectOption(optionValue!)

    // The incident card shows the linked service chip.
    await expect(page.getByRole('button', { name: 'Reopen' })).toBeVisible()
    await expect(page.getByText('Services:', { exact: true })).toBeVisible()
    await expect(page.locator('.linked-chip').filter({ hasText: 'Brake pad replacement' })).toBeVisible()

    // The History tab derives its "Incidents:" chips from the same links.
    await page.getByRole('button', { name: 'History', exact: true }).click()
    await expect(page.getByText('Incidents:', { exact: true })).toBeVisible()
    await expect(page.locator('.linked-chip').filter({ hasText: 'Grinding when braking' })).toBeVisible()
  })

  test('editing an accident records insurance costs', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Incidents' }).click()
    await page.getByRole('button', { name: '+ Add Incident' }).click()
    await page.getByLabel('Category').selectOption('accident')
    await page.getByLabel('Title').fill('Hail damage claim')
    // Financial fields are edit-only (matching the old AccidentsTab).
    await expect(page.getByLabel('Total Repair Cost ($)')).not.toBeVisible()
    await page.getByRole('button', { name: 'Save' }).click()
    await expect(page.getByText('Hail damage claim')).toBeVisible()

    await page.getByText('Hail damage claim').click()
    // Scope to the expanded card: the vehicle header also has an "Edit" button.
    await page.locator('.detail-actions').getByRole('button', { name: 'Edit' }).click()
    await page.getByLabel('Total Repair Cost ($)').fill('2500')
    await page.getByLabel('Deductible ($)').fill('500')
    await page.getByLabel('Insurance Payout ($)').fill('2000')
    await page.getByRole('button', { name: 'Update', exact: true }).click()

    // The still-expanded detail grid shows the amounts.
    await expect(page.getByText('Repair Cost')).toBeVisible()
    await expect(page.getByText('$2500.00')).toBeVisible()
    await expect(page.getByText('$500.00')).toBeVisible()
    await expect(page.getByText('$2000.00')).toBeVisible()
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
