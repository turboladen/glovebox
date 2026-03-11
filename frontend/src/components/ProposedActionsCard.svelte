<script lang="ts">
  import { services, parts, observations } from '../lib/api'
  import type { CreateServiceRecord, CreatePart, CreateObservation } from '../lib/types'

  let { vehicleId, actionsJson }: { vehicleId: number; actionsJson: GloveboxActions } = $props()

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

  // Track creation state per card
  let serviceStates: ('idle' | 'creating' | 'created' | 'error')[] = $state(
    (actionsJson.service_records ?? []).map(() => 'idle')
  )
  let partStates: ('idle' | 'creating' | 'created' | 'error')[] = $state(
    (actionsJson.parts ?? []).map(() => 'idle')
  )
  let obsStates: ('idle' | 'creating' | 'created' | 'error')[] = $state(
    (actionsJson.observations ?? []).map(() => 'idle')
  )

  let creatingAll = $state(false)

  function formatCents(cents: number | null | undefined): string {
    if (cents == null) return ''
    return `$${(cents / 100).toFixed(2)}`
  }

  async function createService(idx: number) {
    const rec = actionsJson.service_records?.[idx]
    if (!rec) return
    serviceStates[idx] = 'creating'
    try {
      const data: CreateServiceRecord = {
        service_date: rec.service_date,
        mileage: rec.mileage ?? undefined,
        description: rec.description ?? undefined,
        parts_cost_cents: rec.parts_cost_cents ?? undefined,
        labor_cost_cents: rec.labor_cost_cents ?? undefined,
        total_cost_cents: rec.total_cost_cents ?? undefined,
        shop_name: rec.shop_name ?? undefined,
        notes: rec.notes ?? undefined,
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
    creatingAll = false
  }

  $effect(() => {
    // Keep state arrays in sync if actionsJson changes
    const sLen = actionsJson.service_records?.length ?? 0
    const pLen = actionsJson.parts?.length ?? 0
    const oLen = actionsJson.observations?.length ?? 0
    if (serviceStates.length !== sLen) serviceStates = Array(sLen).fill('idle')
    if (partStates.length !== pLen) partStates = Array(pLen).fill('idle')
    if (obsStates.length !== oLen) obsStates = Array(oLen).fill('idle')
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
        <span class="all-done">All created!</span>
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

  .all-done {
    font-size: 0.8rem;
    color: var(--success);
    font-weight: 500;
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
</style>
