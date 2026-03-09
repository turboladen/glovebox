<script lang="ts">
  import { vehicles as vehiclesApi, modelTemplates as mtApi } from '../lib/api'
  import type { Vehicle, ModelTemplate } from '../lib/types'
  import { onMount } from 'svelte'

  let {
    vehicle,
    onComplete,
    onCancel,
  }: {
    vehicle: Vehicle
    onComplete: (updated: Vehicle) => void
    onCancel: () => void
  } = $props()

  // Vehicle info fields
  let name = $state(vehicle.name)
  let year = $state<number | undefined>(vehicle.year ?? undefined)
  let make = $state(vehicle.make ?? '')
  let model = $state(vehicle.model ?? '')
  let trimLevel = $state(vehicle.trim_level ?? '')
  let bodyStyle = $state(vehicle.body_style ?? '')
  let engine = $state(vehicle.engine ?? '')
  let transmission = $state(vehicle.transmission ?? '')
  let drivetrain = $state(vehicle.drivetrain ?? '')
  let vin = $state(vehicle.vin ?? '')
  let licensePlate = $state(vehicle.license_plate ?? '')
  let color = $state(vehicle.color ?? '')
  let notes = $state(vehicle.notes ?? '')
  let modelTemplateId = $state<number | undefined>(vehicle.model_template_id ?? undefined)

  // Purchase fields
  let purchaseDate = $state(vehicle.purchase_date ?? '')
  let purchasePriceDollars = $state(vehicle.purchase_price_cents != null ? (vehicle.purchase_price_cents / 100).toFixed(2) : '')
  let purchaseMileage = $state<number | undefined>(vehicle.purchase_mileage ?? undefined)

  // Sold fields
  let soldDate = $state(vehicle.sold_date ?? '')
  let soldPriceDollars = $state(vehicle.sold_price_cents != null ? (vehicle.sold_price_cents / 100).toFixed(2) : '')
  let soldMileage = $state<number | undefined>(vehicle.sold_mileage ?? undefined)

  let templates: ModelTemplate[] = $state([])
  let saving = $state(false)
  let error = $state('')
  let photoInput: HTMLInputElement | undefined = $state(undefined)
  let uploadingPhoto = $state(false)

  onMount(async () => {
    templates = await mtApi.list()
  })

  function parseCents(dollars: string): number | null {
    if (!dollars.trim()) return null
    const n = parseFloat(dollars)
    return isNaN(n) ? null : Math.round(n * 100)
  }

  async function uploadPhoto() {
    const file = photoInput?.files?.[0]
    if (!file) return
    uploadingPhoto = true
    error = ''
    try {
      const updated = await vehiclesApi.uploadPhoto(vehicle.id, file)
      onComplete(updated)
    } catch (e: any) {
      error = e.message
    } finally {
      uploadingPhoto = false
    }
  }

  async function removePhoto() {
    saving = true
    error = ''
    try {
      const updated = await vehiclesApi.update(vehicle.id, { photo_path: null })
      onComplete(updated)
    } catch (e: any) {
      error = e.message
    } finally {
      saving = false
    }
  }

  async function save() {
    if (!name.trim()) {
      error = 'Vehicle name is required'
      return
    }
    const rawPurchase = purchasePriceDollars.trim()
    if (rawPurchase && isNaN(parseFloat(rawPurchase))) {
      error = 'Purchase price must be a valid number'
      return
    }
    const rawSold = soldPriceDollars.trim()
    if (rawSold && isNaN(parseFloat(rawSold))) {
      error = 'Sold price must be a valid number'
      return
    }
    saving = true
    error = ''
    try {
      const purchaseCents = parseCents(purchasePriceDollars)
      const soldCents = parseCents(soldPriceDollars)
      const data: Record<string, unknown> = {
        name: name.trim(),
        year: year ?? null,
        make: make || null,
        model: model || null,
        trim_level: trimLevel || null,
        body_style: bodyStyle || null,
        engine: engine || null,
        transmission: transmission || null,
        drivetrain: drivetrain || null,
        vin: vin || null,
        license_plate: licensePlate || null,
        color: color || null,
        notes: notes || null,
        model_template_id: modelTemplateId ?? null,
        purchase_date: purchaseDate || null,
        purchase_price_cents: purchaseCents,
        purchase_price_currency: purchaseCents != null ? 'USD' : null,
        purchase_mileage: purchaseMileage ?? null,
        sold_date: soldDate || null,
        sold_price_cents: soldCents,
        sold_price_currency: soldCents != null ? 'USD' : null,
        sold_mileage: soldMileage ?? null,
      }
      const updated = await vehiclesApi.update(vehicle.id, data)
      onComplete(updated)
    } catch (e: any) {
      error = e.message
    } finally {
      saving = false
    }
  }
</script>

<div class="vehicle-edit">
  <h3>Edit Vehicle</h3>

  <div class="photo-section">
    <h4>Photo</h4>
    {#if vehicle.photo_path}
      <div class="photo-preview">
        <img src="/files/{vehicle.photo_path}" alt={vehicle.name} />
        <button type="button" class="btn btn-sm btn-secondary" onclick={removePhoto} disabled={saving}>Remove</button>
      </div>
    {/if}
    <div class="photo-upload">
      <input type="file" accept="image/*" bind:this={photoInput} />
      <button type="button" class="btn btn-secondary" onclick={uploadPhoto} disabled={uploadingPhoto}>
        {uploadingPhoto ? 'Uploading...' : vehicle.photo_path ? 'Replace Photo' : 'Upload Photo'}
      </button>
    </div>
  </div>

  <form onsubmit={(e) => { e.preventDefault(); save() }}>
    <div class="field">
      <label for="ve-name">Vehicle Name</label>
      <input id="ve-name" type="text" bind:value={name} required />
    </div>

    <div class="form-row">
      <div class="field">
        <label for="ve-year">Year</label>
        <input id="ve-year" type="number" bind:value={year} min="1900" max="2100" />
      </div>
      <div class="field">
        <label for="ve-make">Make</label>
        <input id="ve-make" type="text" bind:value={make} />
      </div>
      <div class="field">
        <label for="ve-model">Model</label>
        <input id="ve-model" type="text" bind:value={model} />
      </div>
    </div>

    <div class="form-row">
      <div class="field">
        <label for="ve-trim">Trim</label>
        <input id="ve-trim" type="text" bind:value={trimLevel} />
      </div>
      <div class="field">
        <label for="ve-body">Body Style</label>
        <input id="ve-body" type="text" bind:value={bodyStyle} />
      </div>
    </div>

    <div class="form-row">
      <div class="field">
        <label for="ve-engine">Engine</label>
        <input id="ve-engine" type="text" bind:value={engine} />
      </div>
      <div class="field">
        <label for="ve-trans">Transmission</label>
        <input id="ve-trans" type="text" bind:value={transmission} />
      </div>
      <div class="field">
        <label for="ve-drive">Drivetrain</label>
        <input id="ve-drive" type="text" bind:value={drivetrain} />
      </div>
    </div>

    <div class="form-row">
      <div class="field">
        <label for="ve-vin">VIN</label>
        <input id="ve-vin" type="text" bind:value={vin} maxlength="17" />
      </div>
      <div class="field">
        <label for="ve-plate">License Plate</label>
        <input id="ve-plate" type="text" bind:value={licensePlate} />
      </div>
      <div class="field">
        <label for="ve-color">Color</label>
        <input id="ve-color" type="text" bind:value={color} />
      </div>
    </div>

    {#if templates.length > 0}
      <div class="field">
        <label for="ve-template">Model Template</label>
        <select id="ve-template" bind:value={modelTemplateId}>
          <option value={undefined}>None</option>
          {#each templates as t (t.id)}
            <option value={t.id}>
              {t.year ?? ''} {t.make ?? ''} {t.model ?? ''} {t.trim_level ?? ''} ({t.transmission ?? ''})
            </option>
          {/each}
        </select>
      </div>
    {/if}

    <h4>Purchase Info</h4>
    <div class="form-row">
      <div class="field">
        <label for="ve-pdate">Purchase Date</label>
        <input id="ve-pdate" type="date" bind:value={purchaseDate} />
      </div>
      <div class="field">
        <label for="ve-pprice">Purchase Price ($)</label>
        <input id="ve-pprice" type="number" bind:value={purchasePriceDollars} min="0" step="0.01" />
      </div>
      <div class="field">
        <label for="ve-pmileage">Purchase Mileage</label>
        <input id="ve-pmileage" type="number" bind:value={purchaseMileage} min="0" />
      </div>
    </div>

    <h4>Sold Info</h4>
    <div class="form-row">
      <div class="field">
        <label for="ve-sdate">Sold Date</label>
        <input id="ve-sdate" type="date" bind:value={soldDate} />
      </div>
      <div class="field">
        <label for="ve-sprice">Sold Price ($)</label>
        <input id="ve-sprice" type="number" bind:value={soldPriceDollars} min="0" step="0.01" />
      </div>
      <div class="field">
        <label for="ve-smileage">Sold Mileage</label>
        <input id="ve-smileage" type="number" bind:value={soldMileage} min="0" />
      </div>
    </div>

    <div class="field">
      <label for="ve-notes">Notes</label>
      <textarea id="ve-notes" bind:value={notes} rows="3"></textarea>
    </div>

    {#if error}
      <p class="error">{error}</p>
    {/if}
    <div class="form-actions">
      <button type="button" class="btn btn-secondary" onclick={onCancel}>Cancel</button>
      <button type="submit" class="btn btn-primary" disabled={saving}>
        {saving ? 'Saving...' : 'Save Changes'}
      </button>
    </div>
  </form>
</div>

<style>
  .vehicle-edit {
    padding: var(--sp-4) var(--sp-5);
    background: var(--bg-raised);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-lg);
    margin-bottom: var(--sp-5);
  }

  .vehicle-edit h3 {
    margin: 0 0 var(--sp-4);
    font-family: var(--font-display);
  }

  .vehicle-edit h4 {
    margin: var(--sp-4) 0 var(--sp-2);
    font-family: var(--font-display);
    font-size: 0.9rem;
    color: var(--text-muted);
  }

  .form-actions {
    margin-top: var(--sp-4);
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }

  .photo-section {
    margin-bottom: var(--sp-4);
    padding-bottom: var(--sp-4);
    border-bottom: 1px solid var(--border-subtle);
  }

  .photo-section h4 {
    margin: 0 0 var(--sp-2);
    font-family: var(--font-display);
    font-size: 0.9rem;
    color: var(--text-muted);
  }

  .photo-preview {
    display: flex;
    align-items: flex-end;
    gap: var(--sp-3);
    margin-bottom: var(--sp-3);
  }

  .photo-preview img {
    width: 120px;
    height: 80px;
    object-fit: cover;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-subtle);
  }

  .photo-upload {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }
</style>
