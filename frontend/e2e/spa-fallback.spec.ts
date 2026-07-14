import { test, expect } from '@playwright/test'

// TP: Backend SPA fallback — deep links must return HTTP 200 (not 404) so the
// client router boots with a correct status for crawlers/monitoring.
//
// These hit the BACKEND origin (:3003) directly via absolute URLs — the
// Playwright baseURL is the Vite dev server (:5373), which has its own SPA
// fallback, so a relative request would not exercise the backend's behavior.
test.describe('Backend SPA fallback', () => {
  test('serves a deep-link with 200', async ({ request }) => {
    const res = await request.get('http://localhost:3003/vehicles/1/plan')
    expect(res.status()).toBe(200)
    // Stable root marker (Vite doctype casing is not contractual).
    expect(await res.text()).toContain('id="app"')
  })

  test('serves the SPA root with 200', async ({ request }) => {
    // Root is served directly by ServeDir (index.html), not via the deep-link
    // fallback — a regression that broke normal serving would flip this red.
    const res = await request.get('http://localhost:3003/')
    expect(res.status()).toBe(200)
  })
})
