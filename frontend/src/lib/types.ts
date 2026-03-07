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
  notes: string | null
  created_at: string
  updated_at: string
}

export interface ServiceRecordWithLinks extends ServiceRecord {
  schedule_item_ids: number[]
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
  notes?: string | null
  schedule_item_ids?: number[]
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

export interface Setting {
  key: string
  value: string
  created_at: string
  updated_at: string
}
