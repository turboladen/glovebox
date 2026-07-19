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
  delete: (id: number) => request<{ deleted: number }>(`/vehicles/${id}`, { method: 'DELETE' }),
  archive: (id: number) => request<Vehicle>(`/vehicles/${id}/archive`, { method: 'POST' }),
  unarchive: (id: number) => request<Vehicle>(`/vehicles/${id}/unarchive`, { method: 'POST' }),
  uploadPhoto: async (id: number, file: File): Promise<Vehicle> => {
    const formData = new FormData()
    formData.append('file', file)
    const res = await fetch(`${BASE}/vehicles/${id}/photo`, { method: 'POST', body: formData })
    if (!res.ok) {
      const body = await res.json().catch(() => ({ error: res.statusText }))
      throw new Error(body.error || `HTTP ${res.status}`)
    }
    return res.json()
  },
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

// `?documents=` suffix shared by the three entity DELETE calls.
const documentsQuery = (documents?: DocumentDisposition) =>
  documents ? `?documents=${documents}` : ''

// Service Records
export const services = {
  list: (vehicleId: number) => request<ServiceRecordWithLinks[]>(`/vehicles/${vehicleId}/services`),
  get: (vehicleId: number, id: number) => request<ServiceRecordWithLinks>(`/vehicles/${vehicleId}/services/${id}`),
  create: (vehicleId: number, data: CreateServiceRecord) =>
    request<ServiceRecordWithLinks>(`/vehicles/${vehicleId}/services`, { method: 'POST', body: JSON.stringify(data) }),
  update: (vehicleId: number, id: number, data: Partial<CreateServiceRecord>) =>
    request<ServiceRecordWithLinks>(`/vehicles/${vehicleId}/services/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  delete: (vehicleId: number, id: number, documents?: DocumentDisposition) =>
    request<{ deleted: number }>(`/vehicles/${vehicleId}/services/${id}${documentsQuery(documents)}`, {
      method: 'DELETE',
    }),
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
  dismiss: (vehicleId: number, itemId: number, reason?: string) =>
    request<ScheduleItem>(`/vehicles/${vehicleId}/schedule/${itemId}/dismiss`, {
      method: 'POST',
      body: JSON.stringify({ reason: reason ?? null }),
    }),
  undismiss: (vehicleId: number, itemId: number) =>
    request<ScheduleItem>(`/vehicles/${vehicleId}/schedule/${itemId}/dismiss`, { method: 'DELETE' }),
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

// Shops
export const shops = {
  list: () => request<Shop[]>('/shops'),
  get: (id: number) => request<Shop>(`/shops/${id}`),
  create: (data: { name: string; address?: string; phone?: string; website?: string; specialty?: string; notes?: string }) =>
    request<Shop>('/shops', { method: 'POST', body: JSON.stringify(data) }),
  update: (id: number, data: Partial<Shop>) =>
    request<Shop>(`/shops/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  delete: (id: number) =>
    request<{ deleted: number }>(`/shops/${id}`, { method: 'DELETE' }),
}

// Incidents (unified observations + accidents)
export const incidents = {
  list: (vehicleId: number) => request<IncidentWithDetails[]>(`/vehicles/${vehicleId}/incidents`),
  get: (vehicleId: number, id: number) => request<IncidentWithDetails>(`/vehicles/${vehicleId}/incidents/${id}`),
  create: (vehicleId: number, data: CreateIncident) =>
    request<IncidentWithDetails>(`/vehicles/${vehicleId}/incidents`, { method: 'POST', body: JSON.stringify(data) }),
  update: (vehicleId: number, id: number, data: UpdateIncident) =>
    request<IncidentWithDetails>(`/vehicles/${vehicleId}/incidents/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  addFollowup: (vehicleId: number, incidentId: number, data: CreateFollowup) =>
    request<IncidentFollowup>(`/vehicles/${vehicleId}/incidents/${incidentId}/followups`, { method: 'POST', body: JSON.stringify(data) }),
  delete: (vehicleId: number, id: number, documents?: DocumentDisposition) =>
    request<{ deleted: number }>(`/vehicles/${vehicleId}/incidents/${id}${documentsQuery(documents)}`, {
      method: 'DELETE',
    }),
}

// Documents
export const documents = {
  list: (params?: { vehicle_id?: number; linked_entity_type?: string; linked_entity_id?: number }) => {
    const qs = new URLSearchParams()
    if (params?.vehicle_id) qs.set('vehicle_id', String(params.vehicle_id))
    if (params?.linked_entity_type) qs.set('linked_entity_type', params.linked_entity_type)
    if (params?.linked_entity_id) qs.set('linked_entity_id', String(params.linked_entity_id))
    const query = qs.toString()
    return request<Document[]>(`/documents${query ? '?' + query : ''}`)
  },
  upload: async (data: FormData): Promise<Document> => {
    const res = await fetch(`${BASE}/documents`, { method: 'POST', body: data })
    if (!res.ok) {
      const body = await res.json().catch(() => ({ error: res.statusText }))
      throw new Error(body.error || `HTTP ${res.status}`)
    }
    return res.json()
  },
  delete: (id: number) => request<{ deleted: number }>(`/documents/${id}`, { method: 'DELETE' }),
  unlink: (id: number) => request<Document>(`/documents/${id}/unlink`, { method: 'POST' }),
  // Fresh linked-doc count for delete-confirm prompts — never trusts a
  // component's cached document list. Filters by the LINK FIELDS ONLY, the
  // same selector the backend's cascade uses (vehicle_id is nullable on
  // documents), so the prompt count matches what a delete would touch.
  countFor: (linkedEntityType: 'service' | 'part' | 'incident', linkedEntityId: number) =>
    documents
      .list({ linked_entity_type: linkedEntityType, linked_entity_id: linkedEntityId })
      .then((docs) => docs.length),
}

// Parts
export const parts = {
  list: (vehicleId: number, params?: { status?: string }) => {
    const qs = new URLSearchParams()
    if (params?.status) qs.set('status', params.status)
    const query = qs.toString()
    return request<Part[]>(`/vehicles/${vehicleId}/parts${query ? '?' + query : ''}`)
  },
  get: (vehicleId: number, id: number) => request<Part>(`/vehicles/${vehicleId}/parts/${id}`),
  create: (vehicleId: number, data: CreatePart) =>
    request<Part>(`/vehicles/${vehicleId}/parts`, { method: 'POST', body: JSON.stringify(data) }),
  update: (vehicleId: number, id: number, data: Partial<Part>) =>
    request<Part>(`/vehicles/${vehicleId}/parts/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  delete: (vehicleId: number, id: number, documents?: DocumentDisposition) =>
    request<{ deleted: boolean }>(`/vehicles/${vehicleId}/parts/${id}${documentsQuery(documents)}`, {
      method: 'DELETE',
    }),
}

// Costs
export const costs = {
  get: (vehicleId: number) => request<CostSummary>(`/vehicles/${vehicleId}/costs`),
}

// Export
export const vehicleExport = {
  get: (vehicleId: number) => request<VehicleExport>(`/vehicles/${vehicleId}/export`),
}

// Research & Recalls
export const research = {
  checkRecalls: (vehicleId: number) => request<RecallCheckResult>(`/vehicles/${vehicleId}/recalls`),
  listReports: (vehicleId: number) => request<ResearchReport[]>(`/vehicles/${vehicleId}/research`),
  getReport: (vehicleId: number, id: number) => request<ReportWithFindings>(`/vehicles/${vehicleId}/research/${id}`),
  updateFinding: (vehicleId: number, reportId: number, findingId: number, data: { status?: string; linked_entity_type?: string | null; linked_entity_id?: number | null }) =>
    request<ResearchFinding>(`/vehicles/${vehicleId}/research/${reportId}/findings/${findingId}`, {
      method: 'PUT',
      body: JSON.stringify(data),
    }),
  listFindings: (vehicleId: number, status?: string) =>
    request<ResearchFinding[]>(`/vehicles/${vehicleId}/research/findings${status ? `?status=${status}` : ''}`),
}

// Garage-wide dashboard + activity feeds
export const dashboard = {
  get: () => request<GarageDashboard>('/dashboard'),
  activity: (limit?: number) =>
    request<ActivityItem[]>(`/dashboard/activity${limit ? `?limit=${limit}` : ''}`),
  vehicleActivity: (vehicleId: number, limit?: number) =>
    request<ActivityItem[]>(`/vehicles/${vehicleId}/activity${limit ? `?limit=${limit}` : ''}`),
}

// Planning: work items + visits
export const workItems = {
  list: (vehicleId: number, includeDone = false) =>
    request<WorkItem[]>(`/vehicles/${vehicleId}/work-items${includeDone ? '?include_done=true' : ''}`),
  create: (vehicleId: number, data: CreateWorkItem) =>
    request<WorkItem>(`/vehicles/${vehicleId}/work-items`, { method: 'POST', body: JSON.stringify(data) }),
  update: (vehicleId: number, id: number, data: UpdateWorkItem) =>
    request<WorkItem>(`/vehicles/${vehicleId}/work-items/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  delete: (vehicleId: number, id: number) =>
    request<{ deleted: number }>(`/vehicles/${vehicleId}/work-items/${id}`, { method: 'DELETE' }),
}

export const visits = {
  list: (vehicleId: number, includeClosed = false) =>
    request<VisitWithItems[]>(`/vehicles/${vehicleId}/visits${includeClosed ? '?include_closed=true' : ''}`),
  create: (vehicleId: number, data: CreateVisit) =>
    request<VisitWithItems>(`/vehicles/${vehicleId}/visits`, { method: 'POST', body: JSON.stringify(data) }),
  update: (vehicleId: number, id: number, data: UpdateVisit) =>
    request<VisitWithItems>(`/vehicles/${vehicleId}/visits/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  complete: (vehicleId: number, id: number, data: CompleteVisitPayload) =>
    request<CompletedVisit>(`/vehicles/${vehicleId}/visits/${id}/complete`, { method: 'POST', body: JSON.stringify(data) }),
  cancel: (vehicleId: number, id: number) =>
    request<VisitWithItems>(`/vehicles/${vehicleId}/visits/${id}/cancel`, { method: 'POST' }),
  delete: (vehicleId: number, id: number) =>
    request<{ deleted: number }>(`/vehicles/${vehicleId}/visits/${id}`, { method: 'DELETE' }),
}

// Builds
export const builds = {
  list: (vehicleId: number) => request<Build[]>(`/vehicles/${vehicleId}/builds`),
  get: (vehicleId: number, id: number) => request<BuildProgress>(`/vehicles/${vehicleId}/builds/${id}`),
  create: (vehicleId: number, data: { name: string; description?: string | null; target_date?: string | null }) =>
    request<Build>(`/vehicles/${vehicleId}/builds`, { method: 'POST', body: JSON.stringify(data) }),
  update: (vehicleId: number, id: number, data: { name?: string; description?: string | null; target_date?: string | null; status?: string }) =>
    request<Build>(`/vehicles/${vehicleId}/builds/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  delete: (vehicleId: number, id: number) =>
    request<{ deleted: number }>(`/vehicles/${vehicleId}/builds/${id}`, { method: 'DELETE' }),
}

// Budget forecast
export const budget = {
  get: (vehicleId: number) => request<BudgetForecast>(`/vehicles/${vehicleId}/budget`),
}

// Full-text search
export const search = {
  query: (q: string, opts?: { scope?: string; vehicle_id?: number }) => {
    const qs = new URLSearchParams({ q })
    if (opts?.scope) qs.set('scope', opts.scope)
    if (opts?.vehicle_id) qs.set('vehicle_id', String(opts.vehicle_id))
    return request<SearchHit[]>(`/search?${qs}`)
  },
}

// Re-export types for convenience
import type {
  Vehicle, CreateVehicle, Platform, ModelTemplate,
  MileageEntry, CreateMileageEntry,
  ServiceRecordWithLinks, CreateServiceRecord, CreateLineItem,
  ScheduleItem, CreateScheduleItem, ResolvedScheduleItem,
  RemindersResponse, VinDecodeResponse,
  Shop, Document, DocumentDisposition,
  IncidentWithDetails, IncidentFollowup, CreateIncident, UpdateIncident, CreateFollowup,
  Part, CreatePart,
  CostSummary, VehicleExport,
  RecallCheckResult, ResearchReport, ReportWithFindings, ResearchFinding,
  WorkItem, CreateWorkItem, UpdateWorkItem,
  VisitWithItems, CreateVisit, UpdateVisit, CompleteVisitPayload, CompletedVisit,
  Build, BuildProgress, BudgetForecast,
  ActivityItem, GarageDashboard, SearchHit,
} from './types'
