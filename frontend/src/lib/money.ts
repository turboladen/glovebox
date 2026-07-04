/** Money display formatting.
 *
 * Costs are stored as integer cents; display uses integer math only
 * (same convention as the Rust side — no float division for formatting).
 * These are DISPLAY formatters; form prefill keeps plain `(cents/100).toFixed(2)`
 * because number inputs can't hold thousands separators.
 */

/** `$1,234.56` — full-precision display with thousands separators. */
export function formatCents(cents: number): string {
  const sign = cents < 0 ? '-' : ''
  const abs = Math.abs(cents)
  const dollars = Math.floor(abs / 100)
  const rem = abs % 100
  return `${sign}$${dollars.toLocaleString()}.${String(rem).padStart(2, '0')}`
}

/** `$1,235` — whole-dollar display (forecasts, summaries). */
export function formatWholeDollars(cents: number): string {
  const sign = cents < 0 ? '-' : ''
  const abs = Math.abs(cents)
  const dollars = Math.floor(abs / 100) + (abs % 100 >= 50 ? 1 : 0)
  return `${sign}$${dollars.toLocaleString()}`
}
