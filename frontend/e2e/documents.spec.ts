import { test, expect } from '@playwright/test'
import * as path from 'path'
import * as fs from 'fs'
import * as os from 'os'

// TP-15: Documents & Upload
test.describe('Documents', () => {
  let vehicleUrl: string
  let testFilePath: string

  test.beforeAll(async ({ browser }) => {
    // Create a temp file for upload testing
    testFilePath = path.join(os.tmpdir(), 'glovebox-test-upload.txt')
    fs.writeFileSync(testFilePath, 'test document content')

    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Docs Test Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    await page.close()
  })

  test.afterAll(() => {
    if (fs.existsSync(testFilePath)) fs.unlinkSync(testFilePath)
  })

  test('documents tab shows empty state', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Docs' }).click()
    await expect(page.getByText('No documents yet.')).toBeVisible()
    await expect(page.getByRole('button', { name: '+ Upload' })).toBeVisible()
  })

  test('upload a document', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Docs' }).click()
    await page.getByRole('button', { name: '+ Upload' }).click()

    // Set file input
    const fileInput = page.locator('input[type="file"]')
    await fileInput.setInputFiles(testFilePath)
    await page.getByLabel('Title').fill('Test Receipt')
    await page.getByLabel('Type').selectOption('receipt')

    await page.getByRole('button', { name: 'Upload' }).click()

    // Document should appear
    await expect(page.getByText('Test Receipt')).toBeVisible()
    await expect(page.getByText('receipt', { exact: true })).toBeVisible()
  })

  test('delete a document', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Docs' }).click()
    // There should be at least one doc from the upload test
    const deleteButtons = page.getByRole('button', { name: 'Delete' })
    const countBefore = await deleteButtons.count()
    expect(countBefore).toBeGreaterThan(0)
    await deleteButtons.first().click()
    // One fewer delete button after deletion
    await expect(deleteButtons).toHaveCount(countBefore - 1)
  })
})
