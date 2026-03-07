<script lang="ts">
  import { push } from '@keenmate/svelte-spa-router'
  import { vehicles as vehiclesApi, vin as vinApi, modelTemplates as mtApi } from '../lib/api'
  import type { ModelTemplate, VinDecodeResponse } from '../lib/types'
  import { onMount } from 'svelte'

  let step = $state(1)

  // Step 1: VIN
  let vinInput = $state('')
  let vinResult: VinDecodeResponse | null = $state(null)
  let vinError = $state('')
  let vinLoading = $state(false)

  // Step 2: Vehicle details
  let name = $state('')
  let year = $state<number | undefined>()
  let make = $state('')
  let model = $state('')
  let trimLevel = $state('')
  let engine = $state('')
  let transmission = $state('')
  let drivetrain = $state('')
  let purchaseDate = $state('')
  let purchaseMileage = $state<number | undefined>()
  let modelTemplateId = $state<number | undefined>()

  let templates: ModelTemplate[] = $state([])
  let saving = $state(false)
  let error = $state('')

  onMount(async () => {
    templates = await mtApi.list()
  })

  async function decodeVin() {
    if (!vinInput.trim()) return
    vinLoading = true
    vinError = ''
    try {
      vinResult = await vinApi.decode(vinInput.trim())
      // Auto-fill from decoded data
      year = vinResult.year ?? undefined
      make = vinResult.make ?? ''
      model = vinResult.model ?? ''
      trimLevel = vinResult.trim ?? ''
      engine = vinResult.engine ?? ''
      transmission = vinResult.transmission ?? ''
      drivetrain = vinResult.drivetrain ?? ''
      name = `${year ?? ''} ${make} ${model}`.trim()
      step = 2
    } catch (e: any) {
      vinError = e.message
    } finally {
      vinLoading = false
    }
  }

  function skipVin() {
    step = 2
  }

  async function createVehicle() {
    if (!name.trim()) {
      error = 'Vehicle name is required'
      return
    }
    saving = true
    error = ''
    try {
      const vehicle = await vehiclesApi.create({
        name: name.trim(),
        year: year ?? null,
        make: make || null,
        model: model || null,
        trim_level: trimLevel || null,
        engine: engine || null,
        transmission: transmission || null,
        drivetrain: drivetrain || null,
        vin: vinInput || null,
        purchase_date: purchaseDate || null,
        purchase_mileage: purchaseMileage ?? null,
        model_template_id: modelTemplateId ?? null,
      })
      push(`/vehicles/${vehicle.id}`)
    } catch (e: any) {
      error = e.message
    } finally {
      saving = false
    }
  }
</script>

<div class="setup-wizard">
  <h1>Add a Vehicle</h1>

  {#if step === 1}
    <div class="step">
      <h2>Step 1: Enter VIN (optional)</h2>
      <p class="hint">We'll decode the VIN to auto-fill vehicle details using the NHTSA database.</p>
      <div class="field">
        <input
          type="text"
          bind:value={vinInput}
          placeholder="Enter 17-character VIN"
          maxlength="17"
        />
      </div>
      {#if vinError}
        <p class="error">{vinError}</p>
      {/if}
      <div class="form-actions">
        <button class="btn btn-secondary" onclick={skipVin}>Skip</button>
        <button class="btn btn-primary" onclick={decodeVin} disabled={vinLoading || vinInput.length !== 17}>
          {vinLoading ? 'Decoding...' : 'Decode VIN'}
        </button>
      </div>
    </div>
  {:else if step === 2}
    <div class="step">
      <h2>Step 2: Vehicle Details</h2>
      <form onsubmit={(e) => { e.preventDefault(); createVehicle() }}>
        <div class="field">
          <label for="v-name">Vehicle Name</label>
          <input id="v-name" type="text" bind:value={name} required placeholder="e.g., My GTI" />
        </div>

        <div class="form-row">
          <div class="field">
            <label for="v-year">Year</label>
            <input id="v-year" type="number" bind:value={year} min="1900" max="2100" />
          </div>
          <div class="field">
            <label for="v-make">Make</label>
            <input id="v-make" type="text" bind:value={make} />
          </div>
          <div class="field">
            <label for="v-model">Model</label>
            <input id="v-model" type="text" bind:value={model} />
          </div>
        </div>

        <div class="form-row">
          <div class="field">
            <label for="v-trim">Trim</label>
            <input id="v-trim" type="text" bind:value={trimLevel} />
          </div>
          <div class="field">
            <label for="v-engine">Engine</label>
            <input id="v-engine" type="text" bind:value={engine} />
          </div>
        </div>

        <div class="form-row">
          <div class="field">
            <label for="v-trans">Transmission</label>
            <input id="v-trans" type="text" bind:value={transmission} />
          </div>
          <div class="field">
            <label for="v-drive">Drivetrain</label>
            <input id="v-drive" type="text" bind:value={drivetrain} />
          </div>
        </div>

        <div class="form-row">
          <div class="field">
            <label for="v-pdate">Purchase Date</label>
            <input id="v-pdate" type="date" bind:value={purchaseDate} />
          </div>
          <div class="field">
            <label for="v-pmileage">Purchase Mileage</label>
            <input id="v-pmileage" type="number" bind:value={purchaseMileage} min="0" />
          </div>
        </div>

        {#if templates.length > 0}
          <div class="field">
            <label for="v-template">Model Template (for schedule inheritance)</label>
            <select id="v-template" bind:value={modelTemplateId}>
              <option value={undefined}>None</option>
              {#each templates as t (t.id)}
                <option value={t.id}>
                  {t.year ?? ''} {t.make ?? ''} {t.model ?? ''} {t.trim_level ?? ''} ({t.transmission ?? ''})
                </option>
              {/each}
            </select>
          </div>
        {/if}

        {#if error}
          <p class="error">{error}</p>
        {/if}
        <div class="form-actions">
          <button type="button" class="btn btn-secondary" onclick={() => { step = 1 }}>Back</button>
          <button type="submit" class="btn btn-primary" disabled={saving}>
            {saving ? 'Creating...' : 'Create Vehicle'}
          </button>
        </div>
      </form>
    </div>
  {/if}
</div>

<style>
  .setup-wizard {
    max-width: 640px;
  }

  .step {
    margin-top: 1rem;
  }

  .hint {
    font-size: 0.85rem;
    color: var(--text-muted);
    margin-bottom: 1rem;
  }

  .form-row {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 0.75rem;
  }

  .field {
    margin-bottom: 0.75rem;
  }

  .field label {
    display: block;
    font-size: 0.85rem;
    margin-bottom: 0.25rem;
    color: var(--text-muted);
  }

  .field input, .field select {
    width: 100%;
    padding: 0.4rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    font-size: 0.9rem;
    background: var(--bg);
    color: var(--text);
  }

  .form-actions {
    display: flex;
    gap: 0.5rem;
    justify-content: flex-end;
    margin-top: 1rem;
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }
</style>
