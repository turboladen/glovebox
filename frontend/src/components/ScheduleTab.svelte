<script lang="ts">
  import { onMount } from 'svelte'
  import { services as servicesApi, schedules as schedulesApi } from '../lib/api'
  import type { RemindersResponse, ReminderStatus, ServiceRecordWithLinks } from '../lib/types'
  import { formatDate } from '../lib/dates'

  let { reminderData, vehicleId, onScheduleChanged, plannedScheduleIds, onPlanIt }: {
    reminderData: RemindersResponse | null
    vehicleId: number
    onScheduleChanged?: () => Promise<void> | void
    // Schedule items already linked by an open work item (Plan tab wiring).
    plannedScheduleIds?: Set<number>
    onPlanIt?: (reminder: ReminderStatus) => Promise<void> | void
  } = $props()

  let allServices: ServiceRecordWithLinks[] = $state([])
  let expandedItemId: number | null = $state(null)
  let actionError = $state('')

  // Inline minimal-record form (Record service… / Mark done previously)
  let recordTarget: { id: number; name: string; retro: boolean } | null = $state(null)
  let recordDate = $state('')
  let recordOdometer = $state('')
  let recordCost = $state('')
  let recordSaving = $state(false)

  async function loadOwnData() {
    try {
      allServices = await servicesApi.list(vehicleId)
    } catch (e) {
      console.error(e)
    }
  }

  onMount(loadOwnData)

  async function refresh() {
    await loadOwnData()
    await onScheduleChanged?.()
  }

  function openRecordForm(reminder: ReminderStatus, retro: boolean) {
    actionError = ''
    recordTarget = { id: reminder.schedule_item.id, name: reminder.schedule_item.name, retro }
    recordDate = retro ? '' : new Date().toISOString().split('T')[0]
    recordOdometer = ''
    recordCost = ''
  }

  async function submitRecord() {
    if (!recordTarget) return
    recordSaving = true
    actionError = ''
    try {
      await servicesApi.create(vehicleId, {
        service_date: recordDate,
        description: recordTarget.name,
        mileage: recordOdometer ? parseInt(recordOdometer, 10) : undefined,
        total_cost_cents: recordTarget.retro
          ? 0
          : recordCost
            ? Math.round(parseFloat(recordCost) * 100)
            : undefined,
        notes: recordTarget.retro ? 'recorded retroactively' : undefined,
        schedule_item_ids: [recordTarget.id],
      })
      recordTarget = null
      await refresh()
    } catch (e: any) {
      actionError = e.message
    } finally {
      recordSaving = false
    }
  }

  async function dismissItem(reminder: ReminderStatus) {
    actionError = ''
    try {
      await schedulesApi.dismiss(vehicleId, reminder.schedule_item.id)
      await refresh()
    } catch (e: any) {
      actionError = e.message
    }
  }

  function groupByStatus(reminders: ReminderStatus[]) {
    return {
      overdue: reminders.filter((r) => r.status === 'overdue'),
      upcoming: reminders.filter((r) => r.status === 'upcoming'),
      ok: reminders.filter((r) => r.status === 'ok'),
    }
  }

  function servicesForItem(itemId: number): ServiceRecordWithLinks[] {
    return allServices
      .filter(s => s.schedule_item_ids.includes(itemId))
      .sort((a, b) => b.service_date.localeCompare(a.service_date))
  }

  function formatMileage(n: number | null): string {
    return n != null ? n.toLocaleString() : '—'
  }

  function formatCents(cents: number | null): string {
    if (cents == null) return ''
    return `$${(cents / 100).toFixed(2)}`
  }

</script>

{#snippet reminderActions(reminder: ReminderStatus)}
  <div class="reminder-actions">
    {#if onPlanIt}
      {#if plannedScheduleIds?.has(reminder.schedule_item.id)}
        <span class="planned-chip">planned</span>
      {:else}
        <button class="action-link" onclick={() => onPlanIt(reminder)}>Plan it</button>
      {/if}
    {/if}
    <button class="action-link" onclick={() => openRecordForm(reminder, false)}>Record service…</button>
    <button class="action-link" onclick={() => openRecordForm(reminder, true)}>Mark done previously</button>
    <button class="action-link dismiss" onclick={() => dismissItem(reminder)}>Dismiss for this vehicle</button>
  </div>
  {#if recordTarget?.id === reminder.schedule_item.id}
    <form class="record-form" onsubmit={(e) => { e.preventDefault(); submitRecord() }}>
      <div class="record-form-fields">
        <label>
          Date
          <input type="date" bind:value={recordDate} required max={recordTarget.retro ? new Date().toISOString().split('T')[0] : undefined} />
        </label>
        <label>
          Odometer
          <input type="number" min="0" bind:value={recordOdometer} placeholder="optional" />
        </label>
        {#if !recordTarget.retro}
          <label>
            Cost ($)
            <input type="number" step="0.01" min="0" bind:value={recordCost} placeholder="optional" />
          </label>
        {/if}
      </div>
      {#if recordTarget.retro}
        <p class="record-form-hint">Recorded retroactively — pick the date it was actually done.</p>
      {/if}
      <div class="record-form-actions">
        <button type="button" class="action-link" onclick={() => (recordTarget = null)}>Cancel</button>
        <button type="submit" class="btn btn-primary btn-small" disabled={recordSaving || !recordDate}>
          {recordSaving ? 'Saving…' : recordTarget.retro ? 'Save past service' : 'Save service'}
        </button>
      </div>
    </form>
  {/if}
{/snippet}

{#if !reminderData}
  <p>No reminder data available.</p>
{:else}
  {@const groups = groupByStatus(reminderData.reminders)}

  {#if actionError}
    <p class="action-error">{actionError}</p>
  {/if}

  {#if groups.overdue.length > 0}
    <section class="reminder-group">
      <h3 class="group-label overdue-label">Overdue</h3>
      {#each groups.overdue as reminder (reminder.schedule_item.id)}
        <div class="reminder-card overdue">
          <div class="reminder-header">
            <strong>{reminder.schedule_item.name}</strong>
            <span class="trigger">{reminder.trigger}</span>
          </div>
          <div class="reminder-details">
            {#if reminder.due_at_miles}
              <span>Due at {formatMileage(reminder.due_at_miles)} mi</span>
            {/if}
            {#if reminder.due_at_date}
              <span>or {formatDate(reminder.due_at_date)}</span>
            {/if}
          </div>
          <div class="reminder-meta">
            {#if reminder.miles_remaining != null}
              <span>{formatMileage(reminder.miles_remaining)} mi remaining</span>
            {/if}
            {#if reminder.days_remaining != null}
              <span>{reminder.days_remaining} days remaining</span>
            {/if}
          </div>
          {#if reminder.last_service}
            <div class="last-service">
              Last: {formatDate(reminder.last_service.date)}
              {#if reminder.last_service.odometer}@ {formatMileage(reminder.last_service.odometer)} mi{/if}
            </div>
          {:else}
            <div class="last-service">No service recorded</div>
          {/if}
          {#if servicesForItem(reminder.schedule_item.id).length > 0}
            <button class="history-toggle" onclick={() => expandedItemId = expandedItemId === reminder.schedule_item.id ? null : reminder.schedule_item.id}>
              {servicesForItem(reminder.schedule_item.id).length} completion{servicesForItem(reminder.schedule_item.id).length !== 1 ? 's' : ''}
              {expandedItemId === reminder.schedule_item.id ? '▾' : '▸'}
            </button>
          {/if}
          {#if expandedItemId === reminder.schedule_item.id}
            <div class="completion-history">
              {#each servicesForItem(reminder.schedule_item.id) as svc (svc.id)}
                <div class="completion-row">
                  <span>{formatDate(svc.service_date)}</span>
                  {#if svc.mileage}<span>{formatMileage(svc.mileage)} mi</span>{/if}
                  {#if svc.total_cost_cents}<span>{formatCents(svc.total_cost_cents)}</span>{/if}
                  {#if svc.shop_name}<span class="shop">at {svc.shop_name}</span>{/if}
                </div>
              {/each}
            </div>
          {/if}
          {@render reminderActions(reminder)}
        </div>
      {/each}
    </section>
  {/if}

  {#if groups.upcoming.length > 0}
    <section class="reminder-group">
      <h3 class="group-label upcoming-label">Upcoming</h3>
      {#each groups.upcoming as reminder (reminder.schedule_item.id)}
        <div class="reminder-card upcoming">
          <div class="reminder-header">
            <strong>{reminder.schedule_item.name}</strong>
          </div>
          <div class="reminder-details">
            {#if reminder.due_at_miles}
              <span>Due at {formatMileage(reminder.due_at_miles)} mi</span>
            {/if}
            {#if reminder.due_at_date}
              <span>or {formatDate(reminder.due_at_date)}</span>
            {/if}
          </div>
          <div class="reminder-meta">
            {#if reminder.miles_remaining != null}
              <span>~{formatMileage(reminder.miles_remaining)} mi</span>
            {/if}
            {#if reminder.days_remaining != null}
              <span>/ {reminder.days_remaining} days remaining</span>
            {/if}
          </div>
          {#if servicesForItem(reminder.schedule_item.id).length > 0}
            <button class="history-toggle" onclick={() => expandedItemId = expandedItemId === reminder.schedule_item.id ? null : reminder.schedule_item.id}>
              {servicesForItem(reminder.schedule_item.id).length} completion{servicesForItem(reminder.schedule_item.id).length !== 1 ? 's' : ''}
              {expandedItemId === reminder.schedule_item.id ? '▾' : '▸'}
            </button>
          {/if}
          {#if expandedItemId === reminder.schedule_item.id}
            <div class="completion-history">
              {#each servicesForItem(reminder.schedule_item.id) as svc (svc.id)}
                <div class="completion-row">
                  <span>{formatDate(svc.service_date)}</span>
                  {#if svc.mileage}<span>{formatMileage(svc.mileage)} mi</span>{/if}
                  {#if svc.total_cost_cents}<span>{formatCents(svc.total_cost_cents)}</span>{/if}
                  {#if svc.shop_name}<span class="shop">at {svc.shop_name}</span>{/if}
                </div>
              {/each}
            </div>
          {/if}
          {@render reminderActions(reminder)}
        </div>
      {/each}
    </section>
  {/if}

  {#if reminderData.bundle_suggestions.length > 0}
    <section class="bundles">
      <h3 class="group-label bundle-label">Bundle Suggestions</h3>
      {#each reminderData.bundle_suggestions as suggestion}
        <div class="bundle-card">
          <p>{suggestion.reason}</p>
        </div>
      {/each}
    </section>
  {/if}

  {#if groups.ok.length > 0}
    <section class="reminder-group">
      <h3 class="group-label ok-label">OK (not yet due)</h3>
      {#each groups.ok as reminder (reminder.schedule_item.id)}
        <div class="reminder-card ok">
          <div class="reminder-header">
            <strong>{reminder.schedule_item.name}</strong>
          </div>
          <div class="reminder-details">
            {#if reminder.due_at_miles}
              <span>Due at {formatMileage(reminder.due_at_miles)} mi</span>
            {/if}
            {#if reminder.due_at_date}
              <span>or {formatDate(reminder.due_at_date)}</span>
            {/if}
          </div>
          {#if servicesForItem(reminder.schedule_item.id).length > 0}
            <button class="history-toggle" onclick={() => expandedItemId = expandedItemId === reminder.schedule_item.id ? null : reminder.schedule_item.id}>
              {servicesForItem(reminder.schedule_item.id).length} completion{servicesForItem(reminder.schedule_item.id).length !== 1 ? 's' : ''}
              {expandedItemId === reminder.schedule_item.id ? '▾' : '▸'}
            </button>
          {/if}
          {#if expandedItemId === reminder.schedule_item.id}
            <div class="completion-history">
              {#each servicesForItem(reminder.schedule_item.id) as svc (svc.id)}
                <div class="completion-row">
                  <span>{formatDate(svc.service_date)}</span>
                  {#if svc.mileage}<span>{formatMileage(svc.mileage)} mi</span>{/if}
                  {#if svc.total_cost_cents}<span>{formatCents(svc.total_cost_cents)}</span>{/if}
                  {#if svc.shop_name}<span class="shop">at {svc.shop_name}</span>{/if}
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </section>
  {/if}

{/if}

<style>
  .reminder-group {
    margin-bottom: var(--sp-6);
  }

  .group-label {
    font-family: var(--font-display);
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: var(--sp-2);
  }

  .overdue-label { color: var(--danger); }
  .upcoming-label { color: var(--warning); }
  .ok-label { color: var(--success); }
  .bundle-label { color: var(--primary); }

  .reminder-card {
    padding: var(--sp-3) var(--sp-4);
    border-left: 3px solid var(--border-subtle);
    margin-bottom: var(--sp-2);
    background: var(--bg-raised);
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
    transition:
      border-color var(--duration-fast) var(--ease-out),
      background var(--duration-fast) var(--ease-out),
      transform var(--duration-fast) var(--ease-out);
  }

  .reminder-card:hover {
    background: var(--surface);
    transform: translateX(2px);
  }

  .reminder-card.overdue { border-left-color: var(--danger); }
  .reminder-card.upcoming { border-left-color: var(--warning); }
  .reminder-card.ok { border-left-color: var(--success); }

  .reminder-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .trigger {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .reminder-details, .reminder-meta, .last-service {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin-top: var(--sp-1);
  }

  .bundle-card {
    padding: var(--sp-3) var(--sp-4);
    background: var(--bg-raised);
    border-radius: var(--radius-sm);
    border: 1px dashed var(--primary);
    font-size: 0.85rem;
  }

  .bundle-card p {
    margin: 0;
  }

  .history-toggle {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    margin-top: var(--sp-2);
    padding: 0;
    border: none;
    background: none;
    font-size: 0.8rem;
    color: var(--primary);
    cursor: pointer;
    font-weight: 500;
  }

  .history-toggle:hover {
    text-decoration: underline;
  }

  .completion-history {
    margin-top: var(--sp-2);
    padding-left: var(--sp-3);
    border-left: 2px solid var(--border-subtle);
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }

  .completion-row {
    display: flex;
    gap: var(--sp-3);
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .completion-row .shop {
    font-style: italic;
  }

  .reminder-actions {
    display: flex;
    gap: var(--sp-3);
    margin-top: var(--sp-2);
  }

  .action-link {
    padding: 0;
    border: none;
    background: none;
    font-size: 0.8rem;
    color: var(--primary);
    cursor: pointer;
    font-weight: 500;
  }

  .action-link:hover {
    text-decoration: underline;
  }

  .action-link.dismiss {
    color: var(--text-muted);
  }

  .planned-chip {
    font-size: 0.65rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 0 var(--sp-1);
    border-radius: var(--radius-sm);
    background: var(--success-bg);
    color: var(--success);
    border: 1px solid var(--success-border);
    align-self: center;
  }

  .action-error {
    color: var(--danger);
    font-size: 0.85rem;
  }

  .record-form {
    margin-top: var(--sp-3);
    padding: var(--sp-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg);
  }

  .record-form-fields {
    display: flex;
    gap: var(--sp-3);
    flex-wrap: wrap;
  }

  .record-form-fields label {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .record-form-hint {
    margin: var(--sp-2) 0 0;
    font-size: 0.75rem;
    color: var(--text-muted);
    font-style: italic;
  }

  .record-form-actions {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: var(--sp-3);
    margin-top: var(--sp-3);
  }

  .btn-small {
    font-size: 0.8rem;
    padding: var(--sp-1) var(--sp-3);
  }
</style>
