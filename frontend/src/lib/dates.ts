/**
 * Shared date formatting utilities.
 *
 * Backend stores dates as UTC strings in SQLite TEXT columns:
 *   - Datetime: "2025-03-09 14:30:45"
 *   - Date-only: "2025-03-09"
 *
 * Date-only values are treated as local dates (no timezone shift) to avoid
 * off-by-one errors for users west of UTC. Full datetimes are treated as UTC
 * and converted to the user's local timezone for display.
 */

const DATE_ONLY_RE = /^\d{4}-\d{2}-\d{2}$/

/** Parse a backend date string into a Date object.
 *  Date-only strings → local midnight (no timezone shift).
 *  Datetime strings → parsed as UTC, displayed in local timezone. */
function parseDate(dateStr: string): Date | null {
  if (!dateStr) return null
  if (DATE_ONLY_RE.test(dateStr)) {
    // Date-only: parse as local midnight to avoid off-by-one for users west of UTC
    const [y, m, d] = dateStr.split('-').map(Number)
    return new Date(y, m - 1, d)
  }
  // Full datetime: normalize space to T, treat as UTC
  const normalized = dateStr.includes('T') ? dateStr : dateStr.replace(' ', 'T')
  const utc = new Date(normalized + (normalized.includes('Z') ? '' : 'Z'))
  return isNaN(utc.getTime()) ? null : utc
}

/** Format as a short date: "Mar 9, 2025" */
export function formatDate(dateStr: string | null | undefined): string {
  if (!dateStr) return '—'
  const d = parseDate(dateStr)
  if (!d) return dateStr
  return d.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })
}

/** Format as date + time: "Mar 9, 2025, 2:30 PM" */
export function formatDateTime(dateStr: string | null | undefined): string {
  if (!dateStr) return '—'
  const d = parseDate(dateStr)
  if (!d) return dateStr
  return d.toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  })
}

/** Format as a relative description: "Today", "Yesterday", "3 days ago", or falls back to formatDate. */
export function formatRelativeDate(dateStr: string | null | undefined): string {
  if (!dateStr) return '—'
  const d = parseDate(dateStr)
  if (!d) return dateStr
  // Compare calendar dates in local timezone to avoid wall-clock boundary issues
  const now = new Date()
  const todayStart = new Date(now.getFullYear(), now.getMonth(), now.getDate())
  const dStart = new Date(d.getFullYear(), d.getMonth(), d.getDate())
  const diffDays = Math.round((todayStart.getTime() - dStart.getTime()) / 86_400_000)
  if (diffDays === 0) return 'Today'
  if (diffDays === 1) return 'Yesterday'
  if (diffDays > 1 && diffDays < 7) return `${diffDays} days ago`
  return formatDate(dateStr)
}

/** Format a YYYY-MM month string: "March 2025" */
export function formatMonth(monthStr: string | null | undefined): string {
  if (!monthStr) return '—'
  const [year, month] = monthStr.split('-')
  if (!year || !month) return monthStr
  const d = new Date(parseInt(year), parseInt(month) - 1)
  return d.toLocaleDateString(undefined, { year: 'numeric', month: 'long' })
}
