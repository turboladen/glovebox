// Hypermedia deep-link highlight (house convention):
//
// Links that target a specific record append a `?hl=<kind>:<id>` query
// param (e.g. `/vehicles/3/plan/todo?hl=work_item:12`). The target view
// renders each row with an `id="<kind>-<id>"` anchor (underscores become
// dashes: `work-item-12`) via `anchorId()`, and calls
// `flashHighlightFromQuery(kind)` once its rows are in the DOM — the
// matching element scrolls into view and flashes for ~2s (`.hl-flash`
// in global.css; `prefers-reduced-motion` gets a static highlight and an
// instant scroll instead).
import { tick } from 'svelte'
import { querystring } from '@keenmate/svelte-spa-router'

/** The id from `?hl=<kind>:<id>` when the kind matches, else null. */
export function highlightId(kind: string): number | null {
  const hl = new URLSearchParams(querystring() ?? '').get('hl')
  if (!hl) return null
  const [k, idStr] = hl.split(':')
  if (k !== kind) return null
  const id = parseInt(idStr, 10)
  return Number.isNaN(id) ? null : id
}

/** DOM anchor id for a highlightable row: `work_item` + 12 → `work-item-12`. */
export function anchorId(kind: string, id: number): string {
  return `${kind.replace(/_/g, '-')}-${id}`
}

/** Scroll the `?hl=`-named element into view and flash it briefly. */
export async function flashHighlightFromQuery(kind: string): Promise<void> {
  const id = highlightId(kind)
  if (id == null) return
  await tick() // let the caller's freshly loaded rows render first
  const el = document.getElementById(anchorId(kind, id))
  if (!el) return
  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
  el.scrollIntoView({ behavior: reducedMotion ? 'auto' : 'smooth', block: 'center' })
  el.classList.add('hl-flash')
  window.setTimeout(() => el.classList.remove('hl-flash'), 2000)
}
