const BASE = '/api'

async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    headers: { 'Content-Type': 'application/json' },
    ...options,
  })
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw new Error(body.error || `HTTP ${res.status}`)
  }
  return res.json()
}

// Vehicles
export const vehicles = {
  list: () => request<Vehicle[]>('/vehicles'),
  get: (id: number) => request<Vehicle>(`/vehicles/${id}`),
  create: (data: CreateVehicle) => request<Vehicle>('/vehicles', { method: 'POST', body: JSON.stringify(data) }),
  update: (id: number, data: Partial<Vehicle>) => request<Vehicle>(`/vehicles/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
}

// Platforms
export const platforms = {
  list: () => request<Platform[]>('/platforms'),
  get: (id: number) => request<Platform>(`/platforms/${id}`),
}

// Model Templates
export const modelTemplates = {
  list: () => request<ModelTemplate[]>('/model-templates'),
  get: (id: number) => request<ModelTemplate>(`/model-templates/${id}`),
}

// Mileage
export const mileage = {
  list: (vehicleId: number) => request<MileageEntry[]>(`/vehicles/${vehicleId}/mileage`),
  create: (vehicleId: number, data: CreateMileageEntry) =>
    request<MileageEntry>(`/vehicles/${vehicleId}/mileage`, { method: 'POST', body: JSON.stringify(data) }),
}

// Service Records
export const services = {
  list: (vehicleId: number) => request<ServiceRecordWithLinks[]>(`/vehicles/${vehicleId}/services`),
  get: (vehicleId: number, id: number) => request<ServiceRecordWithLinks>(`/vehicles/${vehicleId}/services/${id}`),
  create: (vehicleId: number, data: CreateServiceRecord) =>
    request<ServiceRecordWithLinks>(`/vehicles/${vehicleId}/services`, { method: 'POST', body: JSON.stringify(data) }),
  update: (vehicleId: number, id: number, data: Partial<CreateServiceRecord>) =>
    request<ServiceRecordWithLinks>(`/vehicles/${vehicleId}/services/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
}

// Schedules
export const schedules = {
  list: (params?: { platform_id?: number; model_template_id?: number; vehicle_id?: number }) => {
    const qs = new URLSearchParams()
    if (params?.platform_id) qs.set('platform_id', String(params.platform_id))
    if (params?.model_template_id) qs.set('model_template_id', String(params.model_template_id))
    if (params?.vehicle_id) qs.set('vehicle_id', String(params.vehicle_id))
    const query = qs.toString()
    return request<ScheduleItem[]>(`/schedules${query ? '?' + query : ''}`)
  },
  resolve: (vehicleId: number) => request<ResolvedScheduleItem[]>(`/vehicles/${vehicleId}/schedule`),
  create: (data: CreateScheduleItem) => request<ScheduleItem>('/schedules', { method: 'POST', body: JSON.stringify(data) }),
  update: (id: number, data: Partial<ScheduleItem>) => request<ScheduleItem>(`/schedules/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  delete: (id: number) => request<{ deleted: number }>(`/schedules/${id}`, { method: 'DELETE' }),
}

// Reminders
export const reminders = {
  get: (vehicleId: number) => request<RemindersResponse>(`/vehicles/${vehicleId}/reminders`),
}

// VIN Decode
export const vin = {
  decode: (vinCode: string) => request<VinDecodeResponse>(`/vin/${vinCode}`),
  decodeAndStore: (vehicleId: number, vinCode: string) =>
    request<VinDecodeResponse>(`/vehicles/${vehicleId}/vin-decode/${vinCode}`, { method: 'POST' }),
}

// Settings
export const settings = {
  list: () => request<Setting[]>('/settings'),
  get: (key: string) => request<Setting>(`/settings/${key}`),
  set: (key: string, value: string) => request<Setting>(`/settings/${key}`, { method: 'PUT', body: JSON.stringify({ value }) }),
}

// Re-export types for convenience
import type {
  Vehicle, CreateVehicle, Platform, ModelTemplate,
  MileageEntry, CreateMileageEntry,
  ServiceRecordWithLinks, CreateServiceRecord,
  ScheduleItem, CreateScheduleItem, ResolvedScheduleItem,
  RemindersResponse, VinDecodeResponse, Setting,
} from './types'
