import { test, expect } from '@playwright/test'
import * as path from 'path'
import * as fs from 'fs'
import * as os from 'os'

// TP-24: Invoice PDF Parsing
test.describe('Invoice Parse Button', () => {
  let vehicleUrl: string
  let testTxtPath: string
  let testPdfPath: string

  test.beforeAll(async ({ browser }) => {
    // Create temp files for testing
    testTxtPath = path.join(os.tmpdir(), 'glovebox-test-doc.txt')
    fs.writeFileSync(testTxtPath, 'plain text document')

    // Create a minimal PDF file (just enough for mime detection)
    testPdfPath = path.join(os.tmpdir(), 'glovebox-test-invoice.pdf')
    // Minimal valid PDF
    fs.writeFileSync(testPdfPath, '%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/MediaBox[0 0 612 792]/Parent 2 0 R>>endobj\nxref\n0 4\n0000000000 65535 f \n0000000009 00000 n \n0000000058 00000 n \n0000000115 00000 n \ntrailer<</Size 4/Root 1 0 R>>\nstartxref\n190\n%%EOF')

    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('Invoice Test Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    await page.close()
  })

  test.afterAll(() => {
    if (fs.existsSync(testTxtPath)) fs.unlinkSync(testTxtPath)
    if (fs.existsSync(testPdfPath)) fs.unlinkSync(testPdfPath)
  })

  test('non-PDF document does not show Parse with AI button', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Docs' }).click()

    // Upload a text file
    await page.getByRole('button', { name: '+ Upload' }).click()
    await page.locator('input[type="file"]').setInputFiles(testTxtPath)
    await page.getByLabel('Title').fill('Plain Doc')
    await page.getByRole('button', { name: 'Upload' }).click()
    await expect(page.getByText('Plain Doc')).toBeVisible()

    // No "Parse with AI" button for non-PDF
    await expect(page.getByRole('button', { name: 'Parse with AI' })).not.toBeVisible()
  })

  test('PDF document shows Parse with AI button', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: 'Docs' }).click()

    // Upload a PDF file
    await page.getByRole('button', { name: '+ Upload' }).click()
    await page.locator('input[type="file"]').setInputFiles(testPdfPath)
    await page.getByLabel('Title').fill('Test Invoice')
    await page.getByLabel('Type').selectOption('invoice')
    await page.getByRole('button', { name: 'Upload' }).click()
    await expect(page.getByText('Test Invoice')).toBeVisible()

    // PDF should have the "Parse with AI" button
    await expect(page.getByRole('button', { name: 'Parse with AI' })).toBeVisible()
  })
})
