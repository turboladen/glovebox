export interface Vehicle {
  id: number
  model_template_id: number | null
  name: string
  year: number | null
  make: string | null
  model: string | null
  trim_level: string | null
  body_style: string | null
  engine: string | null
  transmission: string | null
  drivetrain: string | null
  vin: string | null
  license_plate: string | null
  color: string | null
  purchase_date: string | null
  purchase_price_cents: number | null
  purchase_price_currency: string | null
  purchase_mileage: number | null
  sold_date: string | null
  sold_price_cents: number | null
  sold_price_currency: string | null
  sold_mileage: number | null
  photo_path: string | null
  notes: string | null
  created_at: string
  updated_at: string
  archived_at: string | null
}

export interface CreateVehicle {
  name: string
  model_template_id?: number | null
  year?: number | null
  make?: string | null
  model?: string | null
  trim_level?: string | null
  body_style?: string | null
  engine?: string | null
  transmission?: string | null
  drivetrain?: string | null
  vin?: string | null
  license_plate?: string | null
  color?: string | null
  purchase_date?: string | null
  purchase_price_cents?: number | null
  purchase_price_currency?: string | null
  purchase_mileage?: number | null
  photo_path?: string | null
  notes?: string | null
}

export interface Platform {
  id: number
  name: string
  website_url: string | null
  api_base_url: string | null
  notes: string | null
  created_at: string
  updated_at: string
}

export interface ModelTemplate {
  id: number
  platform_id: number | null
  platform_ref: string | null
  year: number | null
  make: string | null
  model: string | null
  trim_level: string | null
  body_style: string | null
  engine: string | null
  transmission: string | null
  drivetrain: string | null
  created_at: string
  updated_at: string
}

export interface MileageEntry {
  id: number
  vehicle_id: number
  mileage: number
  recorded_at: string
  source: string | null
  notes: string | null
  // Set on logs auto-created by a service record (that service already
  // shows the reading — exclude these from merged timelines).
  service_record_id: number | null
  created_at: string
}

export interface CreateMileageEntry {
  mileage: number
  recorded_at?: string
  source?: string
  notes?: string
}

export interface ServiceRecord {
  id: number
  vehicle_id: number
  service_date: string
  mileage: number | null
  description: string | null
  parts_cost_cents: number | null
  parts_cost_currency: string | null
  labor_cost_cents: number | null
  labor_cost_currency: string | null
  total_cost_cents: number | null
  total_cost_currency: string | null
  shop_name: string | null
  shop_id: number | null
  notes: string | null
  paid_by: string
  payer_note: string | null
  created_at: string
  updated_at: string
}

export interface LineItem {
  id: number
  service_record_id: number
  description: string
  category: string | null
  quantity: number | null
  unit_cost_cents: number | null
  cost_cents: number | null
  created_at: string
  updated_at: string
}

export interface CreateLineItem {
  description: string
  category?: string | null
  quantity?: number | null
  unit_cost_cents?: number | null
  cost_cents?: number | null
}

export interface ServiceRecordWithLinks extends ServiceRecord {
  schedule_item_ids: number[]
  part_ids: number[]
  line_items: LineItem[]
}

export interface CreateServiceRecord {
  service_date: string
  mileage?: number | null
  description?: string | null
  parts_cost_cents?: number | null
  parts_cost_currency?: string | null
  labor_cost_cents?: number | null
  labor_cost_currency?: string | null
  total_cost_cents?: number | null
  total_cost_currency?: string | null
  shop_name?: string | null
  shop_id?: number | null
  notes?: string | null
  paid_by?: string
  payer_note?: string | null
  schedule_item_ids?: number[]
  part_ids?: number[]
  line_items?: CreateLineItem[]
}

export interface ScheduleItem {
  id: number
  platform_id: number | null
  model_template_id: number | null
  vehicle_id: number | null
  overrides_item_id: number | null
  name: string
  description: string | null
  interval_miles: number | null
  interval_months: number | null
  labor_categories: string | null
  created_at: string
  updated_at: string
  warning_miles: number | null
  warning_days: number | null
  enabled: boolean
  source: string | null
  notes: string | null
  is_factory_recommended: boolean | null
  // Estimated cost per occurrence (integer cents) — feeds the budget forecast.
  est_cost_cents: number | null
}

export interface CreateScheduleItem {
  platform_id?: number | null
  model_template_id?: number | null
  vehicle_id?: number | null
  overrides_item_id?: number | null
  name: string
  description?: string | null
  interval_miles?: number | null
  interval_months?: number | null
  warning_miles?: number | null
  warning_days?: number | null
  enabled?: boolean
  source?: string | null
  notes?: string | null
  is_factory_recommended?: boolean | null
  labor_categories?: string | null
  est_cost_cents?: number | null
}

export interface ResolvedScheduleItem {
  effective_item: ScheduleItem
  inherited_from: string | null
}

export interface ReminderStatus {
  schedule_item: { id: number; name: string }
  status: 'overdue' | 'upcoming' | 'ok'
  last_service: { id: number; date: string; odometer: number | null } | null
  due_at_miles: number | null
  due_at_date: string | null
  miles_remaining: number | null
  days_remaining: number | null
  trigger: string | null
}

export interface BundleSuggestion {
  reason: string
  items: { id: number; name: string; status: string; due_in_miles: number | null }[]
}

export interface RemindersResponse {
  vehicle_id: number
  estimated_mileage: number
  mileage_is_estimate: boolean
  mileage_as_of: string
  avg_daily_miles: number
  reminders: ReminderStatus[]
  bundle_suggestions: BundleSuggestion[]
}

export interface VinDecodeResponse {
  vin: string
  year: number | null
  make: string | null
  model: string | null
  trim: string | null
  body_style: string | null
  engine: string | null
  transmission: string | null
  drivetrain: string | null
  all_attributes: Record<string, string>
}

export interface Shop {
  id: number
  name: string
  address: string | null
  phone: string | null
  website: string | null
  specialty: string | null
  notes: string | null
  created_at: string
  updated_at: string
}

export interface Document {
  id: number
  vehicle_id: number | null
  title: string
  file_path: string
  file_name: string
  mime_type: string | null
  file_size_bytes: number | null
  doc_type: string | null
  linked_entity_type: string | null
  linked_entity_id: number | null
  notes: string | null
  extracted_text: string | null
  created_at: string
}

export interface Incident {
  id: number
  vehicle_id: number
  category: string
  title: string
  description: string | null
  odometer: number | null
  occurred_at: string
  obd_codes: string | null
  resolved: boolean
  notes: string | null
  fault: string | null
  other_party_name: string | null
  other_party_phone: string | null
  other_party_email: string | null
  other_party_insurance: string | null
  other_party_policy_number: string | null
  insurance_claim_number: string | null
  insurance_adjuster: string | null
  insurance_adjuster_phone: string | null
  total_repair_cost_cents: number | null
  total_repair_cost_currency: string | null
  deductible_cents: number | null
  deductible_currency: string | null
  insurance_payout_cents: number | null
  insurance_payout_currency: string | null
  recurrence_of_id: number | null
  build_id: number | null
  created_at: string
  updated_at: string
}

export interface IncidentFollowup {
  id: number
  incident_id: number
  occurred_at: string
  contact_method: string | null
  contact_with: string | null
  summary: string
  notes: string | null
  created_at: string
}

// The backend flattens the incident into the top level and adds the two
// detail arrays.
export interface IncidentWithDetails extends Incident {
  followups: IncidentFollowup[]
  service_record_ids: number[]
}

export interface CreateIncident {
  category: string
  title: string
  description?: string
  odometer?: number
  occurred_at?: string
  obd_codes?: string
  notes?: string
  fault?: string
  other_party_name?: string
  other_party_phone?: string
  other_party_email?: string
  other_party_insurance?: string
  other_party_policy_number?: string
  insurance_claim_number?: string
  insurance_adjuster?: string
  insurance_adjuster_phone?: string
  recurrence_of_id?: number
  build_id?: number
  service_record_ids?: number[]
}

export interface UpdateIncident {
  category?: string
  title?: string
  description?: string | null
  odometer?: number | null
  occurred_at?: string
  obd_codes?: string | null
  resolved?: boolean
  notes?: string | null
  fault?: string | null
  other_party_name?: string | null
  other_party_phone?: string | null
  other_party_email?: string | null
  other_party_insurance?: string | null
  other_party_policy_number?: string | null
  insurance_claim_number?: string | null
  insurance_adjuster?: string | null
  insurance_adjuster_phone?: string | null
  total_repair_cost_cents?: number | null
  total_repair_cost_currency?: string | null
  deductible_cents?: number | null
  deductible_currency?: string | null
  insurance_payout_cents?: number | null
  insurance_payout_currency?: string | null
  recurrence_of_id?: number | null
  build_id?: number | null
  service_record_ids?: number[]
}

export interface CreateFollowup {
  occurred_at: string
  contact_method?: string
  contact_with?: string
  summary: string
  notes?: string
}

export interface Part {
  id: number
  vehicle_id: number
  name: string
  manufacturer: string | null
  part_number: string | null
  oe_part_number_replaced: string | null
  seller: string | null
  purchase_date: string | null
  cost_cents: number | null
  cost_currency: string | null
  invoice_url: string | null
  status: string
  installed_date: string | null
  installed_odometer: number | null
  installed_service_id: number | null
  replaced_date: string | null
  replaced_odometer: number | null
  notes: string | null
  created_at: string
  updated_at: string
  manufacturer_url: string | null
  retailer_url: string | null
  build_id: number | null
  location: string | null
}

export interface CreatePart {
  name: string
  manufacturer?: string | null
  part_number?: string | null
  oe_part_number_replaced?: string | null
  seller?: string | null
  purchase_date?: string | null
  cost_cents?: number | null
  cost_currency?: string | null
  invoice_url?: string | null
  manufacturer_url?: string | null
  retailer_url?: string | null
  status?: string
  installed_date?: string | null
  installed_odometer?: number | null
  installed_service_id?: number | null
  notes?: string | null
  location?: string | null
}

export interface MonthlyCost {
  month: string
  service_cost_cents: number
  parts_cost_cents: number
  out_of_pocket_cents: number
  covered_cents: number
  total_cents: number
}

export interface CostSummary {
  vehicle_id: number
  total_service_cost_cents: number
  total_parts_cost_cents: number
  total_labor_cost_cents: number
  total_cost_cents: number
  out_of_pocket_cents: number
  covered_cents: number
  service_count: number
  part_count: number
  cost_per_mile_cents: number | null
  monthly_costs: MonthlyCost[]
}

export interface ExportRecord {
  date: string
  mileage: number | null
  description: string | null
  total_cost: string | null
  shop: string | null
  notes: string | null
}

export interface ExportPart {
  name: string
  manufacturer: string | null
  part_number: string | null
  installed_date: string | null
  installed_odometer: number | null
  cost: string | null
}

export interface VehicleExport {
  vehicle_name: string
  year: number | null
  make: string | null
  model: string | null
  vin: string | null
  service_records: ExportRecord[]
  installed_parts: ExportPart[]
  total_service_cost: string
  total_parts_cost: string
  total_cost: string
  record_count: number
}

// Research types
export interface RecallInfo {
  campaign_number: string
  manufacturer: string | null
  subject: string
  summary: string | null
  consequence: string | null
  remedy: string | null
  report_date: string | null
  component: string | null
  action_number: string | null
}

export interface RecallCheckResult {
  make: string
  model: string
  model_year: number
  recall_count: number
  recalls: RecallInfo[]
}

export interface ResearchReport {
  id: number
  vehicle_id: number
  report_type: string | null
  summary: string | null
  raw_data: string | null
  notes: string | null
  generated_at: string
  created_at: string
}

export interface ResearchFinding {
  id: number
  report_id: number
  category: string
  title: string
  description: string | null
  source_url: string | null
  severity: string | null
  status: string
  linked_entity_type: string | null
  linked_entity_id: number | null
  created_at: string
  updated_at: string
}

export interface ReportWithFindings extends ResearchReport {
  findings: ResearchFinding[]
}

// --- Planning (work items + visits, unit G/F) ---

export interface WorkItem {
  id: number
  vehicle_id: number
  title: string
  notes: string | null
  schedule_item_id: number | null
  research_finding_id: number | null
  incident_id: number | null
  build_id: number | null
  est_cost_cents: number | null
  status: string // planned | scheduled | done | dropped
  visit_id: number | null
  created_at: string
  updated_at: string
}

export interface CreateWorkItem {
  title: string
  notes?: string | null
  schedule_item_id?: number | null
  research_finding_id?: number | null
  incident_id?: number | null
  build_id?: number | null
  est_cost_cents?: number | null
  visit_id?: number | null
}

// Update DTO: send null to clear, omit to leave unchanged (double-option).
export interface UpdateWorkItem {
  title?: string
  notes?: string | null
  schedule_item_id?: number | null
  research_finding_id?: number | null
  incident_id?: number | null
  build_id?: number | null
  est_cost_cents?: number | null
  status?: string
  visit_id?: number | null
}

// The backend flattens the visit into the top level and adds items + rollup.
export interface VisitWithItems {
  id: number
  vehicle_id: number
  planned_date: string | null
  shop_name: string | null
  shop_id: number | null
  notes: string | null
  status: string // planned | scheduled | completed | canceled
  service_record_id: number | null
  created_at: string
  updated_at: string
  items: WorkItem[]
  est_total_cents: number
}

export interface CreateVisit {
  planned_date?: string | null
  shop_name?: string | null
  shop_id?: number | null
  notes?: string | null
  work_item_ids?: number[]
}

export interface UpdateVisit {
  planned_date?: string | null
  shop_name?: string | null
  shop_id?: number | null
  notes?: string | null
  status?: string
  // Replace-all attach semantics.
  work_item_ids?: number[]
}

export interface CompleteVisitPayload {
  service_date: string
  mileage?: number | null
  description?: string | null
  total_cost_cents?: number | null
  parts_cost_cents?: number | null
  labor_cost_cents?: number | null
  paid_by?: string | null
  payer_note?: string | null
  notes?: string | null
}

export interface CompletedVisit {
  visit: Omit<VisitWithItems, 'items' | 'est_total_cents'>
  service_record: ServiceRecordWithLinks
  items: WorkItem[]
}

// --- Builds ---

export interface Build {
  id: number
  vehicle_id: number
  name: string
  description: string | null
  status: string // planned | active | on_hold | completed | abandoned
  target_date: string | null
  completed_at: string | null
  created_at: string
  updated_at: string
}

// The backend flattens the build into the top level and adds rollups.
export interface BuildProgress extends Build {
  services_count: number
  parts_total: number
  parts_installed: number
  incidents_count: number
  total_cost_cents: number
  out_of_pocket_cents: number
  linked: {
    service_record_ids: number[]
    part_ids: number[]
    incident_ids: number[]
  }
}

// --- Budget forecast ---

export interface ForecastLine {
  label: string
  when: string
  est_cents: number
}

export interface BudgetForecast {
  horizon_months: number
  projected_maintenance_cents: number
  planned_visits_cents: number
  planned_work_cents: number
  total_cents: number
  lines: ForecastLine[]
}

// --- Activity feed ---

export interface ActivityItem {
  kind: 'service' | 'incident' | 'mileage'
  id: number
  vehicle_id: number
  vehicle_name: string
  date: string
  summary: string
  mileage: number | null
  total_cost_cents: number | null
}

// --- Garage dashboard ---

export interface VehicleSummary {
  vehicle: Vehicle
  estimated_mileage: number | null
  overdue_count: number
  due_soon_count: number
  open_recall_count: number
  unresolved_incident_count: number
  unscheduled_work_count: number
  forecast_total_cents: number
  active_build: { id: number; name: string } | null
}

export interface AttentionItem {
  vehicle_id: number
  vehicle_name: string
  kind: 'overdue' | 'due_soon' | 'recall' | 'incident'
  label: string
  // Raw schedule item name for overdue/due_soon rows (null otherwise) —
  // "plan it" titles the work item from this, not by re-splitting label.
  schedule_item_name: string | null
  entity_id: number
  deep_link_hint: string
  // A participating work item already links this source.
  planned: boolean
}

// Flattened VisitWithItems + the owning vehicle.
export interface UpcomingVisit extends VisitWithItems {
  vehicle_name: string
}

// Flattened BuildProgress + the owning vehicle.
export interface BuildSnapshot extends BuildProgress {
  vehicle_name: string
}

export interface GarageDashboard {
  vehicles: VehicleSummary[]
  attention: AttentionItem[]
  upcoming_visits: UpcomingVisit[]
  budget_total_cents: number
  active_builds: BuildSnapshot[]
}

// --- Search ---

export interface SearchHit {
  kind: 'vehicle' | 'service' | 'incident' | 'incident_followup' | 'build' | 'document' | 'research_finding'
  id: number
  vehicle_id: number | null
  title: string
  snippet: string
  rank: number
}
