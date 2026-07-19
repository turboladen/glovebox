import { test, expect, type Page } from '@playwright/test'
import * as path from 'path'
import * as fs from 'fs'
import * as os from 'os'
import { createVehicle, vehicleIdFrom } from './helpers'

// TP-18: Records → Parts (ported from the retired top-level Parts tab)
test.describe('Records: Parts', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    vehicleUrl = await createVehicle(browser, 'Parts Test Car')
  })

  async function addPart(page: Page, name: string, opts?: { location?: string; cost?: string; seller?: string }) {
    await page.getByRole('button', { name: '+ Add Part' }).click()
    await page.getByLabel('Part Name').fill(name)
    if (opts?.location) await page.getByLabel('Location').fill(opts.location)
    if (opts?.cost) await page.getByLabel('Cost ($)').fill(opts.cost)
    if (opts?.seller) await page.getByLabel('Seller').fill(opts.seller)
    await page.getByRole('button', { name: 'Add Part', exact: true }).click()
    await expect(page.getByText(name)).toBeVisible()
  }

  test('parts sub-view is the Records default and shows empty state', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: /^Records/ }).click()
    await expect(page).toHaveURL(new RegExp(`${vehicleUrl}/records`))
    await expect(page.getByRole('button', { name: 'Parts', exact: true })).toHaveClass(/active/)
    await expect(page.getByText('No parts yet.')).toBeVisible()
    await expect(page.getByRole('button', { name: '+ Add Part' })).toBeVisible()
  })

  test('create a part with a location', async ({ page }) => {
    await page.goto(`${vehicleUrl}/records`)
    await addPart(page, 'GFB DV+', { location: 'Diverter valve', cost: '120.00' })
    await expect(page.getByText('Diverter valve')).toBeVisible()
    await expect(page.getByText('$120.00')).toBeVisible()
    await expect(page.locator('.badge').filter({ hasText: 'purchased' }).first()).toBeVisible()
  })

  test('edit part status to installed', async ({ page }) => {
    await page.goto(`${vehicleUrl}/records`)
    await expect(page.getByText('GFB DV+')).toBeVisible()
    await page.locator('.part-card').filter({ hasText: 'GFB DV+' }).getByRole('button', { name: 'Edit' }).click()
    await page.getByLabel('Status').selectOption('installed')
    await page.getByLabel('Installed Date').fill('2026-01-15')
    await page.getByLabel('Installed Odometer').fill('52000')
    await page.getByRole('button', { name: 'Update Part' }).click()
    await expect(page.locator('.badge').filter({ hasText: 'installed' }).first()).toBeVisible()
  })

  test('edit a part location', async ({ page }) => {
    await page.goto(`${vehicleUrl}/records`)
    await addPart(page, 'Location Edit Part', { location: 'Front brakes' })
    await page.locator('.part-card').filter({ hasText: 'Location Edit Part' }).getByRole('button', { name: 'Edit' }).click()
    // The form pre-fills the existing location.
    await expect(page.getByLabel('Location')).toHaveValue('Front brakes')
    await page.getByLabel('Location').fill('Rear brakes')
    await page.getByRole('button', { name: 'Update Part' }).click()
    await expect(page.getByText('Rear brakes')).toBeVisible()
  })

  test('installed part can create a linked service inline', async ({ page }) => {
    await page.goto(`${vehicleUrl}/records`)
    await page.getByRole('button', { name: '+ Add Part' }).click()
    await page.getByLabel('Part Name').fill('Neuspeed Power Module')
    await page.getByLabel('Seller').fill('ECS Tuning')
    await page.getByLabel('Cost ($)').fill('399.99')
    await page.getByLabel('Status').selectOption('installed')
    await page.getByRole('radio', { name: 'Create new service' }).check()
    await page.getByLabel('Service Date').fill('2026-02-01')
    await page.getByLabel('Description').fill('Power module install')
    await page.getByRole('button', { name: 'Add Part', exact: true }).click()
    await expect(page.getByText('Neuspeed Power Module')).toBeVisible()
    await expect(page.getByText(/via service .*Power module install/)).toBeVisible()
  })

  test('delete a part', async ({ page }) => {
    await page.goto(`${vehicleUrl}/records`)
    await addPart(page, 'Doomed Part')
    const card = page.locator('.part-card').filter({ hasText: 'Doomed Part' })
    await card.getByRole('button', { name: 'Delete', exact: true }).click()
    // No linked documents → the plain 2-way confirm.
    await expect(card.getByText('Delete part "Doomed Part"?')).toBeVisible()
    await card.getByRole('button', { name: 'Yes, Delete' }).click()
    await expect(page.locator('.part-card').filter({ hasText: 'Doomed Part' })).toHaveCount(0)
  })

  // TP-18 + glovebox-9fbj: deleting a part with an attached document offers
  // the 3-way choice; "keep" unlinks the doc instead of leaving it dangling.
  test('delete a part with an attached document keeps the doc on request', async ({ page }) => {
    const docFile = path.join(os.tmpdir(), 'glovebox-part-doc.txt')
    fs.writeFileSync(docFile, 'part invoice content')
    try {
      await page.goto(`${vehicleUrl}/records`)
      await addPart(page, 'Documented Part')

      // Attach a document linked to the part.
      await page.getByRole('button', { name: 'Documents' }).click()
      await page.getByRole('button', { name: '+ Upload' }).click()
      await page.locator('input[type="file"]').setInputFiles(docFile)
      await page.getByLabel('Title').fill('Part Invoice')
      await page.getByLabel('Link to').selectOption('part')
      const option = page.locator('#doc-link-id option', { hasText: 'Documented Part' })
      await page.locator('#doc-link-id').selectOption((await option.getAttribute('value'))!)
      await page.getByRole('button', { name: 'Upload' }).click()
      await expect(page.getByText('Part Invoice')).toBeVisible()

      // Delete the part, keeping the document.
      await page.getByRole('button', { name: 'Parts', exact: true }).click()
      const card = page.locator('.part-card').filter({ hasText: 'Documented Part' })
      await card.getByRole('button', { name: 'Delete', exact: true }).click()
      await expect(card.getByText(/It has 1 attached document\./)).toBeVisible()
      await card.getByRole('button', { name: 'Delete, keep documents' }).click()
      await expect(page.locator('.part-card').filter({ hasText: 'Documented Part' })).toHaveCount(0)

      // The document survives, unlinked, with the provenance note (scoped to
      // its card — other tests on this shared vehicle may add linked docs).
      await page.getByRole('button', { name: 'Documents' }).click()
      const keptCard = page.locator('.doc-card').filter({ hasText: 'Part Invoice' })
      await expect(keptCard).toBeVisible()
      await expect(keptCard.getByText(/Unlinked from part #\d+/)).toBeVisible()
      await expect(keptCard.locator('.doc-link-badge')).toHaveCount(0)
    } finally {
      fs.unlinkSync(docFile)
    }
  })

  // TP-15 + glovebox-9fbj: Unlink clears a document's entity link in place.
  test('Unlink clears a linked document\'s badge and records a note', async ({ page }) => {
    const docFile = path.join(os.tmpdir(), 'glovebox-unlink-doc.txt')
    fs.writeFileSync(docFile, 'unlink me content')
    try {
      // Self-contained: create the part this test links to (tests must not
      // depend on prior test state).
      await page.goto(`${vehicleUrl}/records`)
      await addPart(page, 'Unlink Anchor Part')

      await page.getByRole('button', { name: 'Documents' }).click()
      await page.getByRole('button', { name: '+ Upload' }).click()
      await page.locator('input[type="file"]').setInputFiles(docFile)
      await page.getByLabel('Title').fill('Unlink Me')
      await page.getByLabel('Link to').selectOption('part')
      const option = page.locator('#doc-link-id option', { hasText: 'Unlink Anchor Part' })
      await page.locator('#doc-link-id').selectOption((await option.getAttribute('value'))!)
      await page.getByRole('button', { name: 'Upload' }).click()

      const docCard = page.locator('.doc-card').filter({ hasText: 'Unlink Me' })
      await expect(docCard.locator('.doc-link-badge')).toContainText('Part: Unlink Anchor Part')
      await docCard.getByRole('button', { name: 'Unlink' }).click()
      await expect(docCard.locator('.doc-link-badge')).toHaveCount(0)
      await expect(docCard.getByText(/Unlinked from part #\d+/)).toBeVisible()
    } finally {
      fs.unlinkSync(docFile)
    }
  })

  // TP-19 smoke: parts feed the costs rollup
  test('costs tab shows cost data', async ({ page }) => {
    await page.goto(`${vehicleUrl}/costs`)
    await expect(page.getByText('Cost of Ownership')).toBeVisible()
    await expect(page.getByText('Loading cost data...')).not.toBeVisible()
    // Parts were added in previous tests; we should see Total Spent
    await expect(page.getByText('Total Spent')).toBeVisible()
  })
})

// TP-15: Records → Documents
test.describe('Records: Documents', () => {
  let vehicleUrl: string
  let testFilePath: string

  test.beforeAll(async ({ browser }) => {
    testFilePath = path.join(os.tmpdir(), 'glovebox-test-upload.txt')
    fs.writeFileSync(testFilePath, 'test document content')
    vehicleUrl = await createVehicle(browser, 'Docs Test Car')
  })

  test.afterAll(() => {
    if (fs.existsSync(testFilePath)) fs.unlinkSync(testFilePath)
  })

  test('documents sub-view shows empty state', async ({ page }) => {
    await page.goto(`${vehicleUrl}/records/documents`)
    await expect(page.getByRole('button', { name: 'Documents' })).toHaveClass(/active/)
    await expect(page.getByText('No documents yet.')).toBeVisible()
    await expect(page.getByRole('button', { name: '+ Upload' })).toBeVisible()
  })

  test('upload a document', async ({ page }) => {
    await page.goto(`${vehicleUrl}/records/documents`)
    await page.getByRole('button', { name: '+ Upload' }).click()
    const fileInput = page.locator('input[type="file"]')
    await fileInput.setInputFiles(testFilePath)
    await page.getByLabel('Title').fill('Test Receipt')
    await page.getByLabel('Type').selectOption('receipt')
    await page.getByRole('button', { name: 'Upload' }).click()
    await expect(page.getByText('Test Receipt')).toBeVisible()
    await expect(page.getByText('receipt', { exact: true })).toBeVisible()
  })

  test('delete a document', async ({ page }) => {
    await page.goto(`${vehicleUrl}/records/documents`)
    const deleteButtons = page.getByRole('button', { name: 'Delete' })
    await expect(deleteButtons.first()).toBeVisible({ timeout: 10_000 })
    const countBefore = await deleteButtons.count()
    await deleteButtons.first().click()
    await expect(deleteButtons).toHaveCount(countBefore - 1)
  })
})

// TP-40 (glovebox-alki): Handoff-link attach mode. MCP's record_service
// returns a deep link `?attach=service:<id>`; opening it lands on a
// record-scoped drop zone that uploads a file pre-linked to that service.
test.describe('Records: Documents attach-mode', () => {
  let vehicleUrl: string
  let vehicleId: number
  let testFilePath: string

  test.beforeAll(async ({ browser }) => {
    testFilePath = path.join(os.tmpdir(), 'glovebox-attach-invoice.txt')
    fs.writeFileSync(testFilePath, 'invoice bytes')
    vehicleUrl = await createVehicle(browser, 'Attach Test Car')
    vehicleId = vehicleIdFrom(vehicleUrl)
  })

  test.afterAll(() => {
    if (fs.existsSync(testFilePath)) fs.unlinkSync(testFilePath)
  })

  // Create a service record straight through the API (the same surface MCP's
  // record_service uses) and return its id.
  async function createService(page: Page, description: string): Promise<number> {
    const res = await page.request.post(`/api/vehicles/${vehicleId}/services`, {
      data: { service_date: '2026-06-01', description },
    })
    expect(res.ok()).toBe(true)
    return (await res.json()).id as number
  }

  test('attach URL renders the drop zone and record header', async ({ page }) => {
    const serviceId = await createService(page, 'Timing belt')
    await page.goto(`${vehicleUrl}/records/documents?attach=service:${serviceId}`)

    await expect(page.getByTestId('attach-banner')).toBeVisible()
    await expect(page.getByTestId('attach-banner')).toContainText('Attaching to')
    await expect(page.getByTestId('attach-banner')).toContainText(`Service #${serviceId}`)
    await expect(page.getByTestId('attach-banner')).toContainText('Timing belt')
    await expect(page.getByTestId('dropzone')).toBeVisible()
    // The entity picker is hidden in attach-mode (entity is pre-selected).
    await expect(page.getByLabel('Link to')).toHaveCount(0)
  })

  test('uploading in attach-mode links the doc to the record and shows it on the Timeline', async ({ page }) => {
    const serviceId = await createService(page, 'Clutch replacement')
    await page.goto(`${vehicleUrl}/records/documents?attach=service:${serviceId}`)

    await expect(page.getByTestId('dropzone')).toBeVisible()
    // Drop is flaky to simulate in Playwright — drive the click-fallback input.
    await page.locator('input[type="file"]').setInputFiles(testFilePath)
    await page.getByLabel('Title').fill('Clutch Invoice')
    await page.getByRole('button', { name: 'Upload' }).click()

    // Lands back on the plain documents view (param consumed) with the doc
    // present and linked to the service.
    await expect(page.getByText('Clutch Invoice')).toBeVisible()
    await expect(page.locator('.doc-link-badge').filter({ hasText: 'Clutch replacement' })).toBeVisible()

    // Hypermedia completion: the attached invoice is visible on the Timeline
    // service row as a chip that opens the file.
    await page.goto(`${vehicleUrl}/timeline`)
    const serviceCard = page.locator('.service-card').filter({ hasText: 'Clutch replacement' })
    await expect(serviceCard).toBeVisible()
    await serviceCard.click()
    const docChip = page.getByRole('link', { name: 'Clutch Invoice' })
    await expect(docChip).toBeVisible()
    await expect(docChip).toHaveAttribute('href', /\/files\//)
  })
})

// TP-26/TP-27 moved: Research now lives under Plan → Research (plan.spec.ts).
