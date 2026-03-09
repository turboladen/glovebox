---
name: new-e2e-test
description: |
  Scaffold a Playwright e2e test following glovebox project conventions.
  Encodes async wait patterns, selector best practices, vehicle setup,
  and TEST_PLAN.md integration.
  Trigger on: "new e2e test", "add playwright test", "write e2e", "test this feature".
user_invocable: true
---

# Playwright E2E Test Scaffold

When the user asks for a new e2e test, determine what feature/tab is being tested and create a test file.

## Step 1: Determine Test Scope

Ask the user:
1. **What feature/tab** is being tested? (e.g., "warranty tab", "new form", "existing flow")
2. **Does it need a vehicle?** Most vehicle sub-resource tests do.
3. **TEST_PLAN.md reference**: Check `TEST_PLAN.md` for the relevant test plan ID (e.g., `TP-18`).

## Step 2: Create Test File

File: `frontend/e2e/{feature-name}.spec.ts`

### Template — Vehicle Sub-Resource Tab

```typescript
import { test, expect } from '@playwright/test'

// TP-XX: Feature Name
test.describe('Feature Name', () => {
  let vehicleUrl: string

  test.beforeAll(async ({ browser }) => {
    const page = await browser.newPage()
    await page.goto('/vehicles/new')
    await page.getByRole('button', { name: 'Skip' }).click()
    await page.getByLabel('Vehicle Name').fill('{Feature} Test Car')
    await page.getByRole('button', { name: 'Create Vehicle' }).click()
    await page.waitForURL(/\/vehicles\/\d+/)
    vehicleUrl = new URL(page.url()).pathname
    await page.close()
  })

  test('{feature} tab shows empty state', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: '{Tab Label}' }).click()
    await expect(page.getByText('No {items} yet.')).toBeVisible()
    await expect(page.getByRole('button', { name: '+ Add {Item}' })).toBeVisible()
  })

  test('create a {item}', async ({ page }) => {
    await page.goto(vehicleUrl)
    await page.getByRole('button', { name: '{Tab Label}' }).click()
    await page.getByRole('button', { name: '+ Add {Item}' }).click()
    // Fill form fields...
    await page.getByRole('button', { name: 'Save' }).click()
    await expect(page.getByText('{expected text}')).toBeVisible()
  })

  // Additional tests...
})
```

### Template — Top-Level Feature

```typescript
import { test, expect } from '@playwright/test'

test.describe('Feature Name', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/{route}')
  })

  test('shows expected content', async ({ page }) => {
    await expect(page.getByText('Expected')).toBeVisible()
  })
})
```

## Critical Conventions

### Selector Priority (use in this order)
1. `page.getByRole('button', { name: 'Label' })` — preferred for interactive elements
2. `page.getByRole('textbox', { name: 'Label' })` — for inputs when label is ambiguous (e.g., "Model" vs "Model Template")
3. `page.getByLabel('Label')` — for form fields with unambiguous labels
4. `page.getByPlaceholder('text')` — for placeholder-identified inputs
5. `page.getByText('text')` — for display text assertions
6. `page.locator('.class')` — last resort for CSS-based selection

### Wait Patterns (CRITICAL)
- **ALWAYS** wait for async loading before asserting:
  ```typescript
  // CORRECT: wait for element to appear
  await expect(page.getByText('Data')).toBeVisible()

  // WRONG: checking count immediately (race condition)
  expect(await page.locator('.item').count()).toBe(3)
  ```
- Use `toBeVisible()` not `toHaveCount()` for presence checks
- Use `{ timeout: 10000 }` for slow operations (AI, network calls)
- Wait for loading indicators to disappear: `await expect(page.getByText('Loading...')).not.toBeVisible()`

### Text Matching
- Use `{ exact: true }` when text is a substring of another element:
  ```typescript
  await page.getByText('Observation', { exact: true })  // not "Observations"
  ```
- Use regex for partial URL matching: `await page.waitForURL(/\/vehicles\/\d+/)`

### Vehicle Setup in `beforeAll`
- Create a fresh vehicle per test.describe block — do NOT share across describe blocks
- The VIN skip flow: `await page.getByRole('button', { name: 'Skip' }).click()`
- Always `await page.close()` at end of beforeAll
- Tests within a describe CAN depend on prior test state (e.g., create then edit)
- But each test should navigate fresh: `await page.goto(vehicleUrl)` then click the tab

### Form Interactions
- Currency fields display as dollars: `getByLabel('Cost ($)')` not `getByLabel('Cost')`
- Select dropdowns: `page.getByLabel('Category').selectOption('engine')`
- Date inputs: `page.getByLabel('Date').fill('2026-01-15')` (YYYY-MM-DD format)

### Helper Functions
Extract repeated actions into helper functions within the describe block:
```typescript
async function createItem(page: Page, name: string) {
  await page.getByRole('button', { name: '+ Add Item' }).click()
  await page.getByLabel('Name').fill(name)
  await page.getByRole('button', { name: 'Save' }).click()
  await expect(page.getByText(name)).toBeVisible()
}
```

Import `type Page` from `@playwright/test` if using helper functions.

### History Tab Integration
If the feature should appear in the History tab, add a test:
```typescript
test('{item} appears in history tab', async ({ page }) => {
  await page.goto(vehicleUrl)
  await page.getByRole('button', { name: 'History', exact: true }).click()
  await expect(page.getByText('{item text}')).toBeVisible()
})
```

## Step 3: Update TEST_PLAN.md

Add or update the relevant test plan section in `TEST_PLAN.md` with the new test cases.

## Step 4: Run Tests

```bash
# Single file
cd frontend && bunx playwright test tests/{feature}.spec.ts

# All e2e (requires `just dev` running)
just test-e2e
```

Verify tests pass. If they fail on timing, add appropriate waits — never use `page.waitForTimeout()` as a fix.
