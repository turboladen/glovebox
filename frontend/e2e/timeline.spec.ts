import { test, expect, type Page } from '@playwright/test'
import * as path from 'path'
import * as fs from 'fs'
import * as os from 'os'
import { createVehicle, seedOverdueItem, vehicleIdFrom } from './helpers'

// TP-06 + TP-14: Timeline — the merged stream subsuming History + Incidents
test.describe('Timeline', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    vehicleUrl = await createVehicle(browser, 'Timeline Test Car')
  })

  test('shows empty state', async ({ page }) => {
    await page.goto(`${vehicleUrl}/timeline`)
    await expect(page.getByText('No history yet.')).toBeVisible()
    // ONE record-service verb per screen: the context strip owns it; the
    // Timeline toolbar keeps only the incident verb.
    await expect(page.getByRole('button', { name: 'Record service' })).toHaveCount(1)
    await expect(page.getByRole('button', { name: 'Log incident' })).toBeVisible()
  })

  test('Record service opens the form; its Cancel closes without saving', async ({ page }) => {
    await page.goto(`${vehicleUrl}/timeline`)
    // The strip's verb opens the ONE form on the Timeline…
    await page.getByRole('button', { name: 'Record service' }).click()
    await expect(page.getByLabel('Description')).toBeVisible()
    // …and the form's own Cancel closes it without creating anything.
    await page.locator('form').getByRole('button', { name: 'Cancel' }).click()
    await expect(page.getByLabel('Description')).not.toBeVisible()
    await expect(page.getByText('No history yet.')).toBeVisible()
  })

  test('record a service from the Timeline', async ({ page }) => {
    await page.goto(`${vehicleUrl}/timeline`)
    await page.getByRole('button', { name: 'Record service' }).click()
    await page.getByLabel('Odometer').fill('45200')
    await page.getByLabel('Description').fill('Oil Change')
    await page.getByLabel('Total Cost ($)').fill('49.99')
    await page.getByLabel('Shop').fill('Quick Lube')
    await page.getByRole('button', { name: 'Save Service' }).click()
    await expect(page.getByLabel('Description')).not.toBeVisible()
    await expect(page.getByText('Oil Change')).toBeVisible()
    await expect(page.getByText('$49.99')).toBeVisible()
  })

  test('insurance-paid service shows the costs split', async ({ page }) => {
    await page.goto(vehicleUrl)
    // The header's "Record service" routes to the Timeline with the ONE
    // service form open (the ?action=record param is consumed on arrival).
    await page.getByRole('button', { name: 'Record service' }).click()
    await expect(page.getByRole('heading', { name: 'Record service' })).toBeVisible()
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

  test('log an incident with a category', async ({ page }) => {
    await page.goto(`${vehicleUrl}/timeline`)
    await page.getByRole('button', { name: 'Log incident' }).click()
    await page.getByLabel('Title').fill('Rattle on cold start')
    await page.getByLabel('Category').selectOption('noise')
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(page.getByText('Rattle on cold start')).toBeVisible()
    await expect(page.getByText('noise', { exact: true })).toBeVisible()
  })

  test('accident category reveals insurance fields and shows them on expand', async ({ page }) => {
    await page.goto(`${vehicleUrl}/timeline`)
    await page.getByRole('button', { name: 'Log incident' }).click()
    // The accident-only fieldset is hidden for other categories…
    await expect(page.getByText('Accident Details')).not.toBeVisible()
    // …and revealed when category = accident.
    await page.getByLabel('Category').selectOption('accident')
    await expect(page.getByText('Accident Details')).toBeVisible()
    await page.getByLabel('Title').fill('Sideswiped while parked')
    await page.getByLabel('Other Party Name').fill('J. Doe')
    await page.getByLabel('Claim Number').fill('CLM-4521')
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(page.getByText('Sideswiped while parked')).toBeVisible()
    // Expand the row: the insurance details are shown.
    await page.getByText('Sideswiped while parked').click()
    await expect(page.getByText('CLM-4521')).toBeVisible()
    await expect(page.getByText('J. Doe')).toBeVisible()
  })

  test('add a followup to an incident', async ({ page }) => {
    await page.goto(`${vehicleUrl}/timeline`)
    await page.getByRole('button', { name: 'Log incident' }).click()
    await page.getByLabel('Title').fill('Followup target incident')
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(page.getByText('Followup target incident')).toBeVisible()

    await page.getByText('Followup target incident').click()
    await expect(page.getByText('No followups yet.')).toBeVisible()
    await page.getByRole('button', { name: '+ Add', exact: true }).click()
    await page.getByLabel('Summary').fill('Called the shop about it')
    await page.getByRole('button', { name: 'Add', exact: true }).click()
    await expect(page.getByText('Called the shop about it')).toBeVisible()
  })

  test('resolve toggle marks and reopens an incident', async ({ page }) => {
    await page.goto(`${vehicleUrl}/timeline`)
    await page.getByRole('button', { name: 'Log incident' }).click()
    await page.getByLabel('Title').fill('Resolve toggle incident')
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(page.getByText('Resolve toggle incident')).toBeVisible()

    await page.getByText('Resolve toggle incident').click()
    await page.getByRole('button', { name: 'Mark Resolved' }).click()
    // Resolving offers an optional service link; decline it.
    await page.getByLabel('Link to a service').selectOption({ label: 'Resolve without service' })
    await expect(page.getByRole('button', { name: 'Reopen' })).toBeVisible()

    await page.getByRole('button', { name: 'Reopen' }).click()
    await expect(page.getByRole('button', { name: 'Mark Resolved' })).toBeVisible()
  })

  test('resolving with a service links it and surfaces chips both ways', async ({ page }) => {
    await page.goto(`${vehicleUrl}/timeline`)
    // A service record to link against (the strip owns the one verb).
    await page.getByRole('button', { name: 'Record service' }).click()
    await page.getByLabel('Description').fill('Brake pad replacement')
    await page.getByRole('button', { name: 'Save Service' }).click()
    await expect(page.getByLabel('Description')).not.toBeVisible()

    await page.getByRole('button', { name: 'Log incident' }).click()
    await page.getByLabel('Title').fill('Grinding when braking')
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(page.getByText('Grinding when braking')).toBeVisible()

    // Resolve via the picker, choosing the service.
    await page.getByText('Grinding when braking').click()
    await page.getByRole('button', { name: 'Mark Resolved' }).click()
    const picker = page.getByLabel('Link to a service')
    const optionValue = await picker
      .locator('option', { hasText: 'Brake pad replacement' })
      .getAttribute('value')
    await picker.selectOption(optionValue!)

    // The incident detail shows the linked service chip.
    await expect(page.getByRole('button', { name: 'Reopen' })).toBeVisible()
    await expect(page.getByText('Services:', { exact: true })).toBeVisible()
    await expect(page.locator('.linked-chip').filter({ hasText: 'Brake pad replacement' })).toBeVisible()

    // The service row derives its "Incidents:" chips from the same links
    // (collapse the incident first; scope the click to its title row —
    // the chip on the service card now carries the same text).
    await page.locator('.inc-title', { hasText: 'Grinding when braking' }).click() // collapse
    const serviceCard = page.locator('.service-card', { hasText: 'Brake pad replacement' })
    await expect(serviceCard.getByText('Incidents:', { exact: true })).toBeVisible()
    await expect(serviceCard.locator('.linked-chip').filter({ hasText: 'Grinding when braking' })).toBeVisible()
  })

  test('editing an accident records insurance costs', async ({ page }) => {
    await page.goto(`${vehicleUrl}/timeline`)
    await page.getByRole('button', { name: 'Log incident' }).click()
    await page.getByLabel('Category').selectOption('accident')
    await page.getByLabel('Title').fill('Hail damage claim')
    // Financial fields are edit-only.
    await expect(page.getByLabel('Total Repair Cost ($)')).not.toBeVisible()
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(page.getByText('Hail damage claim')).toBeVisible()

    await page.getByText('Hail damage claim').click()
    // Scope to the expanded detail: the vehicle header also has an "Edit" button.
    await page.locator('.detail-actions').getByRole('button', { name: 'Edit' }).click()
    await page.getByLabel('Total Repair Cost ($)').fill('2500')
    await page.getByLabel('Deductible ($)').fill('500')
    await page.getByLabel('Insurance Payout ($)').fill('2000')
    await page.getByRole('button', { name: 'Update', exact: true }).click()

    // The row stayed expanded; the detail grid shows the amounts.
    await expect(page.getByText('Repair Cost')).toBeVisible()
    await expect(page.getByText('$2,500.00')).toBeVisible()
    await expect(page.getByText('$500.00')).toBeVisible()
    await expect(page.getByText('$2,000.00')).toBeVisible()
  })

  test('filter chips narrow the stream by kind', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'Timeline Filter Car')
    const vehicleId = parseInt(url.split('/').pop()!, 10)
    // One of each kind via the API.
    for (const [path, data] of [
      ['services', { service_date: '2026-05-01', description: 'Filter svc row' }],
      ['incidents', { category: 'noise', title: 'Filter incident row' }],
      ['mileage', { mileage: 61_000 }],
    ] as const) {
      const res = await page.request.post(`/api/vehicles/${vehicleId}/${path}`, { data })
      expect(res.ok()).toBe(true)
    }

    await page.goto(`${url}/timeline`)
    // Scope to the stream — the sidebar also shows the car's mileage.
    const list = page.locator('.history-list')
    await expect(list.getByText('Filter svc row')).toBeVisible()
    await expect(list.getByText('Filter incident row')).toBeVisible()
    await expect(list.getByText('61,000 mi')).toBeVisible()

    await page.getByRole('button', { name: 'Services', exact: true }).click()
    await expect(list.getByText('Filter svc row')).toBeVisible()
    await expect(list.getByText('Filter incident row')).not.toBeVisible()

    await page.getByRole('button', { name: 'Incidents', exact: true }).click()
    await expect(list.getByText('Filter incident row')).toBeVisible()
    await expect(list.getByText('Filter svc row')).not.toBeVisible()

    await page.getByRole('button', { name: 'Mileage', exact: true }).click()
    await expect(list.getByText('61,000 mi')).toBeVisible()
    await expect(list.getByText('Filter incident row')).not.toBeVisible()
  })

  test('Incidents filter reveals category chips that narrow by category', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'Timeline Category Car')
    const vehicleId = parseInt(url.split('/').pop()!, 10)
    for (const [category, title] of [
      ['noise', 'Category noise row'],
      ['leak', 'Category leak row'],
    ] as const) {
      const res = await page.request.post(`/api/vehicles/${vehicleId}/incidents`, {
        data: { category, title },
      })
      expect(res.ok()).toBe(true)
    }

    await page.goto(`${url}/timeline`)
    const list = page.locator('.history-list')
    // No category chips until the kind filter is Incidents.
    await expect(page.getByTestId('category-filter')).not.toBeVisible()
    await page.getByRole('button', { name: 'Incidents', exact: true }).click()
    const chips = page.getByTestId('category-filter')
    await expect(chips).toBeVisible()

    await chips.getByRole('button', { name: 'noise' }).click()
    await expect(list.getByText('Category noise row')).toBeVisible()
    await expect(list.getByText('Category leak row')).not.toBeVisible()

    await chips.getByRole('button', { name: 'leak' }).click()
    await expect(list.getByText('Category leak row')).toBeVisible()
    await expect(list.getByText('Category noise row')).not.toBeVisible()

    await chips.getByRole('button', { name: 'All' }).click()
    await expect(list.getByText('Category noise row')).toBeVisible()
    await expect(list.getByText('Category leak row')).toBeVisible()
  })

  // Import reconciliation from the record side: an existing service row can
  // be linked to a maintenance item from the Timeline; the linked name
  // deep-links to Plan → Due, where the reminder now shows Last done.
  test('link a service to a maintenance item from the Timeline', async ({ browser, page }) => {
    const url = await createVehicle(browser, 'Timeline Link Car')
    const vehicleId = vehicleIdFrom(url)
    await seedOverdueItem(page, vehicleId, 'Coolant flush')

    // An import-style record: created without any schedule link.
    const res = await page.request.post(`/api/vehicles/${vehicleId}/services`, {
      data: { service_date: '2026-06-01', description: 'Coolant drain and fill' },
    })
    expect(res.ok()).toBe(true)

    // Expand the row; the picker lists the vehicle's schedule items.
    await page.goto(`${url}/timeline`)
    await page.getByText('Coolant drain and fill').click()
    await page.getByRole('button', { name: 'Link to maintenance item…' }).click()
    const picker = page.getByTestId('maintenance-picker')
    await expect(picker).toBeVisible()
    await picker.getByRole('button', { name: 'Coolant flush' }).click()

    // The linked item renders as a chip that deep-links to Plan → Due…
    const chip = page.getByRole('link', { name: 'Coolant flush' })
    await expect(chip).toBeVisible()
    await chip.click()

    // …where the reminder state updated: no longer overdue, Last done set
    // (and pointing back at the record we linked).
    await expect(page).toHaveURL(new RegExp(`/vehicles/${vehicleId}/plan/due`))
    await expect(page.getByRole('heading', { name: 'OK (not yet due)' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'Overdue' })).toHaveCount(0)
    await expect(page.getByText(/Last done/)).toBeVisible()
    await expect(page.getByText('Coolant drain and fill')).toBeVisible()
  })
})

// TP-06 + glovebox-9fbj: deleting a service/incident with the
// attached-documents choice. "Keep" unlinks the docs (with a provenance
// note); "Delete + documents" cascades rows and files.
test.describe('Timeline: delete with documents', () => {
  let vehicleUrl: string
  const tmpFiles: string[] = []

  function tmpFile(name: string, content: string): string {
    const p = path.join(os.tmpdir(), name)
    fs.writeFileSync(p, content)
    tmpFiles.push(p)
    return p
  }

  test.beforeAll(async ({ browser }) => {
    vehicleUrl = await createVehicle(browser, 'Delete Docs Car')
  })

  test.afterAll(() => {
    for (const f of tmpFiles) if (fs.existsSync(f)) fs.unlinkSync(f)
  })

  async function recordService(page: Page, description: string) {
    await page.goto(`${vehicleUrl}/timeline`)
    await page.getByRole('button', { name: 'Record service' }).click()
    await page.getByLabel('Description').fill(description)
    await page.getByRole('button', { name: 'Save Service' }).click()
    await expect(page.getByLabel('Description')).not.toBeVisible()
    await expect(page.getByText(description)).toBeVisible()
  }

  async function attachDocToService(page: Page, title: string, serviceDescription: string, filePath: string) {
    await page.goto(`${vehicleUrl}/records/documents`)
    await page.getByRole('button', { name: '+ Upload' }).click()
    await page.locator('input[type="file"]').setInputFiles(filePath)
    await page.getByLabel('Title').fill(title)
    await page.getByLabel('Link to').selectOption('service')
    const option = page.locator('#doc-link-id option', { hasText: serviceDescription })
    await page.locator('#doc-link-id').selectOption((await option.getAttribute('value'))!)
    await page.getByRole('button', { name: 'Upload' }).click()
    await expect(page.getByText(title)).toBeVisible()
  }

  test('service with no documents gets the plain confirm', async ({ page }) => {
    await recordService(page, 'Plain doomed service')
    await page.getByText('Plain doomed service').click()
    await page.getByRole('button', { name: 'Delete', exact: true }).click()
    await expect(page.getByText('Delete this record?')).toBeVisible()
    await page.getByRole('button', { name: 'Yes, Delete' }).click()
    await expect(page.getByText('Plain doomed service')).not.toBeVisible()
  })

  test('keeping documents unlinks them with a provenance note', async ({ page }) => {
    await recordService(page, 'Keep docs service')
    await attachDocToService(
      page,
      'Kept Invoice',
      'Keep docs service',
      tmpFile('glovebox-keep-doc.txt', 'kept invoice content'),
    )

    await page.goto(`${vehicleUrl}/timeline`)
    await page.getByText('Keep docs service').click()
    await page.getByRole('button', { name: 'Delete', exact: true }).click()
    await expect(page.getByText(/It has 1 attached document\./)).toBeVisible()
    await page.getByRole('button', { name: 'Delete, keep documents' }).click()
    await expect(page.getByText('Keep docs service')).not.toBeVisible()

    // The document survives, unlinked, note visible.
    await page.goto(`${vehicleUrl}/records/documents`)
    const docCard = page.locator('.doc-card').filter({ hasText: 'Kept Invoice' })
    await expect(docCard).toBeVisible()
    await expect(docCard.getByText(/Unlinked from service #\d+/)).toBeVisible()
    await expect(docCard.locator('.doc-link-badge')).toHaveCount(0)
  })

  test('cascade delete removes the documents too', async ({ page }) => {
    await recordService(page, 'Cascade docs service')
    await attachDocToService(
      page,
      'Doomed Invoice',
      'Cascade docs service',
      tmpFile('glovebox-cascade-doc.txt', 'doomed invoice content'),
    )

    await page.goto(`${vehicleUrl}/timeline`)
    await page.getByText('Cascade docs service').click()
    await page.getByRole('button', { name: 'Delete', exact: true }).click()
    await page.getByRole('button', { name: 'Delete + documents' }).click()
    await expect(page.getByText('Cascade docs service')).not.toBeVisible()

    await page.goto(`${vehicleUrl}/records/documents`)
    await expect(page.getByText('Loading documents...')).not.toBeVisible()
    await expect(page.getByText('Doomed Invoice')).not.toBeVisible()
  })

  test('a failed delete surfaces the error and keeps the confirm open', async ({ page }) => {
    await recordService(page, 'Unkillable service')
    await page.route('**/api/vehicles/*/services/*', (route) =>
      route.request().method() === 'DELETE'
        ? route.fulfill({ status: 500, contentType: 'application/json', body: JSON.stringify({ error: 'boom' }) })
        : route.fallback(),
    )
    await page.getByText('Unkillable service').click()
    await page.getByRole('button', { name: 'Delete', exact: true }).click()
    await page.getByRole('button', { name: 'Yes, Delete' }).click()
    // The confirm row stays open with the backend's error; the record remains.
    await expect(page.getByRole('alert')).toContainText('boom')
    await expect(page.getByRole('button', { name: 'Yes, Delete' })).toBeVisible()
    // Cancel collapses the row AND clears the stale error.
    await page.getByRole('button', { name: 'Cancel' }).click()
    await expect(page.getByRole('alert')).toHaveCount(0)
    await expect(page.getByText('Unkillable service')).toBeVisible()
    await page.unroute('**/api/vehicles/*/services/*')
  })

  test('delete an incident from its detail panel', async ({ page }) => {
    await page.goto(`${vehicleUrl}/timeline`)
    await page.getByRole('button', { name: 'Log incident' }).click()
    await page.getByLabel('Title').fill('Doomed incident')
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    await expect(page.getByText('Doomed incident')).toBeVisible()

    await page.getByText('Doomed incident').click()
    const actions = page.locator('.detail-actions')
    await actions.getByRole('button', { name: 'Delete', exact: true }).click()
    await expect(page.getByText('Delete this incident?')).toBeVisible()
    await actions.getByRole('button', { name: 'Yes, Delete' }).click()
    await expect(page.getByText('Doomed incident')).not.toBeVisible()
  })
})
