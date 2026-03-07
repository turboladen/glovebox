<script lang="ts">
  import { onMount } from 'svelte'
  import { link } from '@keenmate/svelte-spa-router'
  import { vehicles as vehiclesApi, reminders as remindersApi, mileage as mileageApi } from '../lib/api'
  import type { Vehicle, RemindersResponse } from '../lib/types'
  import ScheduleTab from './ScheduleTab.svelte'
  import HistoryTab from './HistoryTab.svelte'
  import MileageEntry from './MileageEntry.svelte'
  import ServiceForm from './ServiceForm.svelte'

  let { routeParams = {} }: { routeParams?: Record<string, string> } = $props()

  let vehicle: Vehicle | null = $state(null)
  let reminderData: RemindersResponse | null = $state(null)
  let loading = $state(true)
  let error = $state('')
  let activeTab = $state('schedule')
  let showMileageForm = $state(false)
  let showServiceForm = $state(false)

  async function loadData() {
    try {
      const id = parseInt(routeParams.id)
      vehicle = await vehiclesApi.get(id)
      reminderData = await remindersApi.get(id)
    } catch (e: any) {
      error = e.message
    } finally {
      loading = false
    }
  }

  onMount(loadData)

  async function onMileageAdded() {
    showMileageForm = false
    if (vehicle) {
      reminderData = await remindersApi.get(vehicle.id)
    }
  }

  async function onServiceAdded() {
    showServiceForm = false
    if (vehicle) {
      reminderData = await remindersApi.get(vehicle.id)
    }
  }

  function formatMileage(n: number): string {
    return n.toLocaleString()
  }
</script>

{#if loading}
  <p class="loading">Loading...</p>
{:else if error}
  <p class="error">{error}</p>
{:else if vehicle}
  <div class="vehicle-detail">
    <div class="detail-header">
      <a href="/" use:link class="back-link">← Garage</a>
      <h1>{vehicle.name}</h1>
    </div>

    <div class="status-bar">
      {#if reminderData}
        <span class="est-mileage">{formatMileage(reminderData.estimated_mileage)} mi (est.)</span>
        <span class="mileage-date">as of {reminderData.mileage_as_of}</span>
      {/if}
      <div class="actions">
        <button class="btn btn-secondary" onclick={() => (showMileageForm = !showMileageForm)}>
          Update Mileage
        </button>
        <button class="btn btn-primary" onclick={() => (showServiceForm = !showServiceForm)}>
          Log Service
        </button>
      </div>
    </div>

    {#if showMileageForm}
      <MileageEntry vehicleId={vehicle.id} onComplete={onMileageAdded} onCancel={() => (showMileageForm = false)} />
    {/if}

    {#if showServiceForm}
      <ServiceForm vehicleId={vehicle.id} onComplete={onServiceAdded} onCancel={() => (showServiceForm = false)} />
    {/if}

    <div class="tabs">
      <button class="tab" class:active={activeTab === 'schedule'} onclick={() => (activeTab = 'schedule')}>
        Schedule
      </button>
      <button class="tab" class:active={activeTab === 'history'} onclick={() => (activeTab = 'history')}>
        History
      </button>
    </div>

    <div class="tab-content">
      {#if activeTab === 'schedule'}
        <ScheduleTab {reminderData} vehicleId={vehicle.id} />
      {:else if activeTab === 'history'}
        <HistoryTab vehicleId={vehicle.id} />
      {/if}
    </div>
  </div>
{/if}

<style>
  .detail-header {
    margin-bottom: 1rem;
  }

  .back-link {
    font-size: 0.85rem;
    color: var(--text-muted);
    text-decoration: none;
  }

  .back-link:hover {
    color: var(--text);
  }

  .detail-header h1 {
    margin: 0.25rem 0 0;
  }

  .status-bar {
    display: flex;
    align-items: center;
    gap: 1rem;
    flex-wrap: wrap;
    margin-bottom: 1rem;
    padding: 0.75rem 1rem;
    background: var(--surface);
    border-radius: 8px;
  }

  .est-mileage {
    font-weight: 600;
    font-size: 1.1rem;
  }

  .mileage-date {
    font-size: 0.85rem;
    color: var(--text-muted);
  }

  .actions {
    margin-left: auto;
    display: flex;
    gap: 0.5rem;
  }

  .tabs {
    display: flex;
    gap: 0;
    border-bottom: 2px solid var(--border);
    margin-bottom: 1rem;
  }

  .tab {
    padding: 0.5rem 1rem;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    margin-bottom: -2px;
    cursor: pointer;
    font-size: 0.9rem;
    color: var(--text-muted);
  }

  .tab.active {
    color: var(--text);
    border-bottom-color: var(--primary);
    font-weight: 600;
  }

  .tab:hover:not(.active) {
    color: var(--text);
  }

  .loading, .error {
    padding: 2rem 0;
  }

  .error {
    color: var(--danger);
  }
</style>
