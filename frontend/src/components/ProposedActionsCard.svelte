<script lang="ts">
  import { services, parts, observations, shops as shopsApi, reminders as remindersApi } from '../lib/api'
  import type { CreateServiceRecord, CreateLineItem, CreatePart, CreateObservation, Shop, ReminderStatus } from '../lib/types'
  import { formatDate } from '../lib/dates'

  let { vehicleId, actionsJson, alreadyCreated = false, onActionsCreated, onNavigate }: {
    vehicleId: number
    actionsJson: GloveboxActions
    alreadyCreated?: boolean
    onActionsCreated?: (summary: string) => void
    onNavigate?: (tab: string) => void
  } = $props()

  interface GloveboxActions {
    service_records?: ServiceRecordAction[]
    parts?: PartAction[]
    observations?: ObservationAction[]
  }

  interface ServiceRecordAction {
    service_date: string
    mileage?: number | null
    description?: string | null
    parts_cost_cents?: number | null
    labor_cost_cents?: number | null
    total_cost_cents?: number | null
    shop_name?: string | null
    notes?: string | null
    schedule_item_ids?: number[] | null
    line_items?: CreateLineItem[] | null
  }

  interface PartAction {
    name: string
    manufacturer?: string | null
    part_number?: string | null
    status?: string
    installed_date?: string | null
    installed_odometer?: number | null
    cost_cents?: number | null
    seller?: string | null
    notes?: string | null
  }

  interface ObservationAction {
    category: string
    title: string
    description?: string | null
    odometer?: number | null
    obd_codes?: string | null
    notes?: string | null
  }

  // Track creation state per card — synced from props via $effect.pre below
  let serviceStates: ('idle' | 'creating' | 'created' | 'error')[] = $state([])
  let partStates: ('idle' | 'creating' | 'created' | 'error')[] = $state([])
  let obsStates: ('idle' | 'creating' | 'created' | 'error')[] = $state([])

  let creatingAll = $state(false)
  let shopList: Shop[] | null = $state(null)
  let createdShopNames: string[] = $state([])
  let remindersByItemId: Map<number, ReminderStatus> | null = $state(null)

  // Per-service-record: which schedule_item_ids the user has confirmed (default: all proposed)
  let scheduleSelections: Map<number, Set<number>> = $state(new Map())

  // Initialize schedule selections from proposed data
  function initScheduleSelections() {
    const selections = new Map<number, Set<number>>()
    for (let i = 0; i < (actionsJson.service_records?.length ?? 0); i++) {
      const ids = actionsJson.service_records![i].schedule_item_ids
      if (ids?.length) {
        selections.set(i, new Set(ids))
      }
    }
    scheduleSelections = selections
  }
  // Initialize schedule selections and load reminder data reactively
  $effect(() => {
    initScheduleSelections()
  })

  $effect(() => {
    const hasLinks = (actionsJson.service_records ?? []).some(r => r.schedule_item_ids?.length)
    if (hasLinks && !alreadyCreated) {
      ensureReminders()
    }
  })

  async function ensureReminders(): Promise<Map<number, ReminderStatus>> {
    if (remindersByItemId == null) {
      const resp = await remindersApi.get(vehicleId)
      const map = new Map<number, ReminderStatus>()
      for (const r of resp.reminders) {
        map.set(r.schedule_item.id, r)
      }
      remindersByItemId = map
    }
    return remindersByItemId
  }

  function scheduleItemName(id: number): string {
    return remindersByItemId?.get(id)?.schedule_item.name ?? `Schedule item #${id}`
  }

  function scheduleItemDetail(id: number): string {
    const r = remindersByItemId?.get(id)
    if (!r) return ''
    const parts: string[] = []
    if (r.status === 'overdue') {
      parts.push('OVERDUE')
    } else if (r.status === 'upcoming') {
      parts.push('upcoming')
    }
    if (r.last_service) {
      const lastDate = formatDate(r.last_service.date)
      const lastOdo = r.last_service.odometer
      parts.push(`last: ${lastDate}${lastOdo ? ` @ ${lastOdo.toLocaleString()} mi` : ''}`)
    } else {
      parts.push('never serviced')
    }
    if (r.due_at_miles != null) {
      parts.push(`due @ ${r.due_at_miles.toLocaleString()} mi`)
    } else if (r.due_at_date) {
      parts.push(`due ${formatDate(r.due_at_date)}`)
    }
    return parts.join(' · ')
  }

  function toggleScheduleItem(serviceIdx: number, itemId: number) {
    const current = scheduleSelections.get(serviceIdx) ?? new Set()
    const updated = new Set(current)
    if (updated.has(itemId)) {
      updated.delete(itemId)
    } else {
      updated.add(itemId)
    }
    scheduleSelections = new Map(scheduleSelections).set(serviceIdx, updated)
  }

  async function ensureShops(): Promise<Shop[]> {
    if (shopList == null) {
      shopList = await shopsApi.list()
    }
    return shopList
  }

  function formatCents(cents: number | null | undefined): string {
    if (cents == null) return ''
    return `$${(cents / 100).toFixed(2)}`
  }

  async function resolveShop(shopName: string): Promise<{ shop_name: string; shop_id?: number }> {
    const shops = await ensureShops()
    const match = shops.find(s => s.name.toLowerCase() === shopName.toLowerCase())
    if (match) {
      return { shop_name: match.name, shop_id: match.id }
    }
    // Shop doesn't exist — create it
    const created = await shopsApi.create({ name: shopName })
    shopList = [...(shopList ?? []), created]
    createdShopNames = [...createdShopNames, created.name]
    return { shop_name: created.name, shop_id: created.id }
  }

  async function createService(idx: number) {
    const rec = actionsJson.service_records?.[idx]
    if (!rec) return
    serviceStates[idx] = 'creating'
    try {
      let shopFields: { shop_name?: string; shop_id?: number } = {}
      if (rec.shop_name) {
        shopFields = await resolveShop(rec.shop_name)
      }

      const confirmedScheduleIds = scheduleSelections.get(idx)
      const data: CreateServiceRecord = {
        service_date: rec.service_date,
        mileage: rec.mileage ?? undefined,
        description: rec.description ?? undefined,
        parts_cost_cents: rec.parts_cost_cents ?? undefined,
        labor_cost_cents: rec.labor_cost_cents ?? undefined,
        total_cost_cents: rec.total_cost_cents ?? undefined,
        shop_name: shopFields.shop_name ?? undefined,
        shop_id: shopFields.shop_id ?? undefined,
        notes: rec.notes ?? undefined,
        schedule_item_ids: confirmedScheduleIds?.size ? [...confirmedScheduleIds] : undefined,
        line_items: rec.line_items?.length ? rec.line_items : undefined,
      }
      await services.create(vehicleId, data)
      serviceStates[idx] = 'created'
    } catch {
      serviceStates[idx] = 'error'
    }
  }

  async function createPart(idx: number) {
    const p = actionsJson.parts?.[idx]
    if (!p) return
    partStates[idx] = 'creating'
    try {
      const data: CreatePart = {
        name: p.name,
        manufacturer: p.manufacturer ?? undefined,
        part_number: p.part_number ?? undefined,
        status: p.status ?? 'installed',
        installed_date: p.installed_date ?? undefined,
        installed_odometer: p.installed_odometer ?? undefined,
        cost_cents: p.cost_cents ?? undefined,
        seller: p.seller ?? undefined,
        notes: p.notes ?? undefined,
      }
      await parts.create(vehicleId, data)
      partStates[idx] = 'created'
    } catch {
      partStates[idx] = 'error'
    }
  }

  async function createObs(idx: number) {
    const o = actionsJson.observations?.[idx]
    if (!o) return
    obsStates[idx] = 'creating'
    try {
      const data: CreateObservation = {
        category: o.category,
        title: o.title,
        description: o.description ?? undefined,
        odometer: o.odometer ?? undefined,
        obd_codes: o.obd_codes ?? undefined,
        notes: o.notes ?? undefined,
      }
      await observations.create(vehicleId, data)
      obsStates[idx] = 'created'
    } catch {
      obsStates[idx] = 'error'
    }
  }

  async function createAll() {
    creatingAll = true
    const promises: Promise<void>[] = []
    for (let i = 0; i < (actionsJson.service_records?.length ?? 0); i++) {
      if (serviceStates[i] === 'idle') promises.push(createService(i))
    }
    for (let i = 0; i < (actionsJson.parts?.length ?? 0); i++) {
      if (partStates[i] === 'idle') promises.push(createPart(i))
    }
    for (let i = 0; i < (actionsJson.observations?.length ?? 0); i++) {
      if (obsStates[i] === 'idle') promises.push(createObs(i))
    }
    await Promise.all(promises)

    const summary = buildSummary()
    if (summary && onActionsCreated) {
      onActionsCreated(summary)
    }

    creatingAll = false
  }

  function buildSummary(): string {
    const lines: string[] = []
    for (const name of createdShopNames) {
      lines.push(` \u2022 ${name} (shop)`)
    }
    for (let i = 0; i < (actionsJson.service_records?.length ?? 0); i++) {
      if (serviceStates[i] === 'created') {
        lines.push(` \u2022 ${actionsJson.service_records![i].description ?? 'Service record'} (service)`)
        const linkedIds = scheduleSelections.get(i)
        if (linkedIds?.size) {
          for (const id of linkedIds) {
            lines.push(`    \u25E6 ${scheduleItemName(id)} (schedule fulfilled)`)
          }
        }
      }
    }
    for (let i = 0; i < (actionsJson.parts?.length ?? 0); i++) {
      if (partStates[i] === 'created') {
        lines.push(` \u2022 ${actionsJson.parts![i].name} (part)`)
      }
    }
    for (let i = 0; i < (actionsJson.observations?.length ?? 0); i++) {
      if (obsStates[i] === 'created') {
        lines.push(` \u2022 ${actionsJson.observations![i].title} (observation)`)
      }
    }
    return lines.join('\n')
  }

  $effect.pre(() => {
    // Keep state arrays in sync with actionsJson (and initialize on first render)
    const ds = alreadyCreated ? 'created' : 'idle'
    const sLen = actionsJson.service_records?.length ?? 0
    const pLen = actionsJson.parts?.length ?? 0
    const oLen = actionsJson.observations?.length ?? 0
    if (serviceStates.length !== sLen) serviceStates = Array(sLen).fill(ds)
    if (partStates.length !== pLen) partStates = Array(pLen).fill(ds)
    if (obsStates.length !== oLen) obsStates = Array(oLen).fill(ds)
  })

  const hasAny = $derived(
    (actionsJson.service_records?.length ?? 0) > 0 ||
    (actionsJson.parts?.length ?? 0) > 0 ||
    (actionsJson.observations?.length ?? 0) > 0
  )

  const allCreated = $derived(
    serviceStates.every(s => s === 'created') &&
    partStates.every(s => s === 'created') &&
    obsStates.every(s => s === 'created')
  )

  const anyIdle = $derived(
    serviceStates.some(s => s === 'idle') ||
    partStates.some(s => s === 'idle') ||
    obsStates.some(s => s === 'idle')
  )
</script>

{#if hasAny}
  <div class="proposed-actions">
    <div class="actions-header">
      <span class="actions-title">Proposed Records</span>
      {#if allCreated}
        <span class="all-done-row">
          <span class="all-done">All created!</span>
          {#if onNavigate}
            <button class="btn btn-sm view-link" onclick={() => onNavigate?.('history')}>View in History</button>
          {/if}
        </span>
      {:else if anyIdle}
        <button class="btn btn-primary btn-sm" onclick={createAll} disabled={creatingAll}>
          {creatingAll ? 'Creating...' : 'Create All'}
        </button>
      {/if}
    </div>

    {#each actionsJson.service_records ?? [] as rec, i (i)}
      <div class="action-card" class:created={serviceStates[i] === 'created'}>
        <div class="card-badge">Service Record</div>
        <div class="card-fields">
          <span><strong>Date:</strong> {rec.service_date}</span>
          {#if rec.mileage}<span><strong>Mileage:</strong> {rec.mileage.toLocaleString()}</span>{/if}
          {#if rec.description}<span><strong>Description:</strong> {rec.description}</span>{/if}
          {#if rec.total_cost_cents != null}<span><strong>Total:</strong> {formatCents(rec.total_cost_cents)}</span>{/if}
          {#if rec.parts_cost_cents != null}<span><strong>Parts:</strong> {formatCents(rec.parts_cost_cents)}</span>{/if}
          {#if rec.labor_cost_cents != null}<span><strong>Labor:</strong> {formatCents(rec.labor_cost_cents)}</span>{/if}
          {#if rec.shop_name}<span><strong>Shop:</strong> {rec.shop_name}</span>{/if}
          {#if rec.notes}<span><strong>Notes:</strong> {rec.notes}</span>{/if}
          {#if rec.line_items?.length}
            <div class="line-items-list">
              <span class="line-items-label">Line Items:</span>
              {#each rec.line_items as li}
                <div class="line-item-row">
                  {#if li.category}<span class="line-item-cat">{li.category}</span>{/if}
                  <span class="line-item-desc">{li.description}</span>
                  {#if li.quantity != null}<span class="line-item-qty">x{li.quantity}</span>{/if}
                  {#if li.cost_cents != null}<span class="line-item-cost">{formatCents(li.cost_cents)}</span>{/if}
                </div>
              {/each}
            </div>
          {/if}
          {#if rec.schedule_item_ids?.length && remindersByItemId}
            <div class="schedule-links">
              <span class="schedule-label">Fulfills schedule:</span>
              {#each rec.schedule_item_ids as itemId (itemId)}
                {@const selected = scheduleSelections.get(i)?.has(itemId) ?? false}
                {@const detail = scheduleItemDetail(itemId)}
                <label class="schedule-toggle" class:disabled={serviceStates[i] === 'created'}>
                  <input
                    type="checkbox"
                    checked={selected}
                    disabled={serviceStates[i] === 'created'}
                    onchange={() => toggleScheduleItem(i, itemId)}
                  />
                  <span class="schedule-info">
                    <span class="schedule-name">{scheduleItemName(itemId)}</span>
                    {#if detail}
                      <span class="schedule-detail">{detail}</span>
                    {/if}
                  </span>
                </label>
              {/each}
            </div>
          {/if}
        </div>
        <div class="card-action">
          {#if serviceStates[i] === 'created'}
            <span class="status-created">Created</span>
          {:else if serviceStates[i] === 'error'}
            <span class="status-error">Failed</span>
            <button class="btn btn-sm" onclick={() => createService(i)}>Retry</button>
          {:else}
            <button class="btn btn-sm" onclick={() => createService(i)} disabled={serviceStates[i] === 'creating'}>
              {serviceStates[i] === 'creating' ? '...' : 'Create'}
            </button>
          {/if}
        </div>
      </div>
    {/each}

    {#each actionsJson.parts ?? [] as p, i (i)}
      <div class="action-card" class:created={partStates[i] === 'created'}>
        <div class="card-badge">Part</div>
        <div class="card-fields">
          <span><strong>Name:</strong> {p.name}</span>
          {#if p.manufacturer}<span><strong>Manufacturer:</strong> {p.manufacturer}</span>{/if}
          {#if p.part_number}<span><strong>Part #:</strong> {p.part_number}</span>{/if}
          {#if p.cost_cents != null}<span><strong>Cost:</strong> {formatCents(p.cost_cents)}</span>{/if}
          {#if p.seller}<span><strong>Seller:</strong> {p.seller}</span>{/if}
          {#if p.installed_date}<span><strong>Installed:</strong> {p.installed_date}</span>{/if}
          {#if p.installed_odometer != null}<span><strong>At:</strong> {p.installed_odometer.toLocaleString()} mi</span>{/if}
        </div>
        <div class="card-action">
          {#if partStates[i] === 'created'}
            <span class="status-created">Created</span>
          {:else if partStates[i] === 'error'}
            <span class="status-error">Failed</span>
            <button class="btn btn-sm" onclick={() => createPart(i)}>Retry</button>
          {:else}
            <button class="btn btn-sm" onclick={() => createPart(i)} disabled={partStates[i] === 'creating'}>
              {partStates[i] === 'creating' ? '...' : 'Create'}
            </button>
          {/if}
        </div>
      </div>
    {/each}

    {#each actionsJson.observations ?? [] as obs, i (i)}
      <div class="action-card" class:created={obsStates[i] === 'created'}>
        <div class="card-badge">Observation</div>
        <div class="card-fields">
          <span><strong>Category:</strong> {obs.category}</span>
          <span><strong>Title:</strong> {obs.title}</span>
          {#if obs.description}<span><strong>Description:</strong> {obs.description}</span>{/if}
          {#if obs.odometer != null}<span><strong>Odometer:</strong> {obs.odometer.toLocaleString()}</span>{/if}
          {#if obs.obd_codes}<span><strong>OBD Codes:</strong> {obs.obd_codes}</span>{/if}
        </div>
        <div class="card-action">
          {#if obsStates[i] === 'created'}
            <span class="status-created">Created</span>
          {:else if obsStates[i] === 'error'}
            <span class="status-error">Failed</span>
            <button class="btn btn-sm" onclick={() => createObs(i)}>Retry</button>
          {:else}
            <button class="btn btn-sm" onclick={() => createObs(i)} disabled={obsStates[i] === 'creating'}>
              {obsStates[i] === 'creating' ? '...' : 'Create'}
            </button>
          {/if}
        </div>
      </div>
    {/each}
  </div>
{/if}

<style>
  .proposed-actions {
    margin-top: var(--sp-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-raised);
    padding: var(--sp-3);
  }

  .actions-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--sp-3);
  }

  .actions-title {
    font-weight: 600;
    font-size: 0.85rem;
    font-family: var(--font-display);
  }

  .all-done-row {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }

  .all-done {
    font-size: 0.8rem;
    color: var(--success);
    font-weight: 500;
  }

  .view-link {
    font-size: 0.75rem;
    color: var(--primary);
    text-decoration: underline;
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
  }

  .view-link:hover {
    color: var(--primary-hover, var(--primary));
  }

  .action-card {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: var(--sp-2) var(--sp-3);
    margin-bottom: var(--sp-2);
    display: flex;
    align-items: flex-start;
    gap: var(--sp-3);
    background: var(--bg-base);
    transition: opacity 0.2s ease;
  }

  .action-card.created {
    opacity: 0.6;
  }

  .action-card:last-child {
    margin-bottom: 0;
  }

  .card-badge {
    font-size: 0.65rem;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--primary);
    background: var(--primary-subtle, var(--surface));
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: var(--sp-0) var(--sp-2);
    white-space: nowrap;
    font-family: var(--font-display);
    flex-shrink: 0;
    margin-top: 2px;
  }

  .card-fields {
    flex: 1;
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-1) var(--sp-3);
    font-size: 0.8rem;
  }

  .card-action {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex-shrink: 0;
  }

  .btn-sm {
    font-size: 0.75rem;
    padding: var(--sp-1) var(--sp-2);
  }

  .status-created {
    font-size: 0.75rem;
    color: var(--success);
    font-weight: 500;
  }

  .status-error {
    font-size: 0.75rem;
    color: var(--danger);
    font-weight: 500;
  }

  .line-items-list {
    width: 100%;
    margin-top: var(--sp-1);
    padding-top: var(--sp-1);
    border-top: 1px dashed var(--border-subtle);
  }

  .line-items-label {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .line-item-row {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: 0.8rem;
    padding: 1px 0;
  }

  .line-item-cat {
    font-size: 0.65rem;
    font-weight: 500;
    text-transform: uppercase;
    color: var(--text-muted);
    background: var(--surface);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: 0 var(--sp-1);
    flex-shrink: 0;
  }

  .line-item-desc {
    flex: 1;
  }

  .line-item-qty {
    color: var(--text-muted);
    font-size: 0.75rem;
    flex-shrink: 0;
  }

  .line-item-cost {
    font-weight: 500;
    flex-shrink: 0;
  }

  .schedule-links {
    width: 100%;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-1) var(--sp-3);
    margin-top: var(--sp-1);
    padding-top: var(--sp-1);
    border-top: 1px dashed var(--border-subtle);
  }

  .schedule-label {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .schedule-toggle {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-1);
    font-size: 0.8rem;
    cursor: pointer;
    width: 100%;
  }

  .schedule-toggle.disabled {
    opacity: 0.6;
    cursor: default;
  }

  .schedule-toggle input[type="checkbox"] {
    width: auto;
    margin: 2px 0 0;
    flex-shrink: 0;
  }

  .schedule-info {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .schedule-name {
    font-weight: 500;
  }

  .schedule-detail {
    font-size: 0.7rem;
    color: var(--text-muted);
  }
</style>
