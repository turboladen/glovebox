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
  created_at: string
  updated_at: string
}

export interface ServiceRecordWithLinks extends ServiceRecord {
  schedule_item_ids: number[]
  part_ids: number[]
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
  schedule_item_ids?: number[]
  part_ids?: number[]
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

export interface Observation {
  id: number
  vehicle_id: number
  category: string
  title: string
  description: string | null
  odometer: number | null
  observed_at: string
  obd_codes: string | null
  resolved: boolean
  resolved_service_id: number | null
  notes: string | null
  created_at: string
  updated_at: string
}

export interface CreateObservation {
  category: string
  title: string
  description?: string
  odometer?: number
  observed_at?: string
  obd_codes?: string
  notes?: string
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

export interface Accident {
  id: number
  vehicle_id: number
  occurred_at: string
  odometer: number | null
  description: string
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
  resolved: boolean
  notes: string | null
  created_at: string
  updated_at: string
}

export interface AccidentCorrespondence {
  id: number
  accident_id: number
  occurred_at: string
  contact_method: string | null
  contact_with: string | null
  summary: string
  notes: string | null
  created_at: string
}

export interface AccidentWithDetails extends Accident {
  correspondence: AccidentCorrespondence[]
  service_record_ids: number[]
}

export interface CreateAccident {
  occurred_at: string
  odometer?: number | null
  description: string
  fault?: string | null
  other_party_name?: string | null
  other_party_phone?: string | null
  other_party_email?: string | null
  other_party_insurance?: string | null
  other_party_policy_number?: string | null
  insurance_claim_number?: string | null
  insurance_adjuster?: string | null
  insurance_adjuster_phone?: string | null
  notes?: string | null
  service_record_ids?: number[]
}

export interface UpdateAccident {
  occurred_at?: string
  odometer?: number | null
  description?: string
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
  resolved?: boolean
  notes?: string | null
  service_record_ids?: number[]
}

export interface CreateCorrespondence {
  occurred_at: string
  contact_method?: string | null
  contact_with?: string | null
  summary: string
  notes?: string | null
}

export interface PartSlot {
  id: number
  vehicle_id: number
  name: string
  category: string | null
  oe_spec: string | null
  oe_part_number: string | null
  notes: string | null
  created_at: string
}

export interface CreatePartSlot {
  name: string
  category?: string | null
  oe_spec?: string | null
  oe_part_number?: string | null
  notes?: string | null
}

export interface Part {
  id: number
  slot_id: number | null
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
}

export interface CreatePart {
  slot_id?: number | null
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
}

export interface MonthlyCost {
  month: string
  service_cost_cents: number
  parts_cost_cents: number
  total_cents: number
}

export interface CostSummary {
  vehicle_id: number
  total_service_cost_cents: number
  total_parts_cost_cents: number
  total_labor_cost_cents: number
  total_cost_cents: number
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

// AI Provider types
export interface AiProvider {
  id: number
  name: string
  provider_type: string
  api_key_set: boolean
  api_base: string | null
  model: string | null
  is_default: boolean
  enabled: boolean
  created_at: string
  updated_at: string
}

export interface CreateAiProvider {
  name: string
  provider_type: string
  api_key?: string | null
  api_base?: string | null
  model?: string | null
  is_default?: boolean
  enabled?: boolean
}

export interface UpdateAiProvider {
  name?: string
  provider_type?: string
  api_key?: string | null
  api_base?: string | null
  model?: string | null
  is_default?: boolean
  enabled?: boolean
}

export interface ProviderSummary {
  id: number
  name: string
  provider_type: string
  is_default: boolean
  enabled: boolean
}

// AI types
export interface AiStatus {
  provider: string
  configured: boolean
  default_provider_id: number | null
  providers: ProviderSummary[]
}

export interface Conversation {
  id: number
  vehicle_id: number | null
  title: string
  created_at: string
  updated_at: string
}

export interface ChatMessage {
  id: number
  vehicle_id: number | null
  role: 'user' | 'assistant'
  content: string
  created_at: string
  conversation_id: number | null
}

export interface ChatResponse {
  message: ChatMessage
  input_tokens: number | null
  output_tokens: number | null
}

export interface ParsedInvoice {
  service_date: string | null
  shop_name: string | null
  mileage: number | null
  description: string | null
  line_items: { description: string; cost_cents: number | null }[]
  parts_cost_cents: number | null
  labor_cost_cents: number | null
  total_cost_cents: number | null
  notes: string | null
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

export interface ModelInfo {
  id: string
  display_name: string | null
}

export interface AiSuggestion {
  title: string
  reason: string
  urgency: 'high' | 'medium' | 'low'
  estimated_cost_range: string | null
}
