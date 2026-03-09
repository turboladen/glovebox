<script lang="ts">
  import { onMount } from 'svelte'
  import { services as servicesApi } from '../lib/api'
  import type { RemindersResponse, ReminderStatus, ServiceRecordWithLinks } from '../lib/types'
  import { formatDate } from '../lib/dates'
  import SuggestionsCard from './SuggestionsCard.svelte'

  let { reminderData, vehicleId }: { reminderData: RemindersResponse | null; vehicleId: number } = $props()

  let allServices: ServiceRecordWithLinks[] = $state([])
  let expandedItemId: number | null = $state(null)

  onMount(async () => {
    try {
      allServices = await servicesApi.list(vehicleId)
    } catch (e) {
      console.error(e)
    }
  })

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

<SuggestionsCard {vehicleId} />

{#if !reminderData}
  <p>No reminder data available.</p>
{:else}
  {@const groups = groupByStatus(reminderData.reminders)}

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
</style>
