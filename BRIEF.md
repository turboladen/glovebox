# Glovebox — Project Brief & Architecture Plan

A personal car maintenance tracker and ownership record system. Built to solve the immediate problem of "what does my car need and when," and designed to grow into a comprehensive vehicle ownership companion with AI-powered features.

## Owner Context

- **User:** Steve, software developer, Rust-first, comfortable with Ruby/JS/TS/Python
- **Current cars:**
  - 2017 Volkswagen Golf GTI SE (~55k miles) — modded (GFB DV, intercooler upgrade planned)
  - 2017 Volkswagen Golf Alltrack SEL (~115k miles) — stock, high mileage, needs attention
- **Both cars** are on the MQB platform with EA888 engines. The GTI has a 6-speed manual transmission (FWD), the Alltrack has DSG + AWD (Haldex)
- **Running custom intervals** — shorter than factory for some things (e.g., oil), factory for others
- **Related projects:** Homie (home management MCP server, similar entity/event/document concepts but standalone), Financier (finance app, future integration target)

---

## Tech Stack

| Layer | Choice | Notes |
|-------|--------|-------|
| Backend | **Axum** (Rust) | REST API, serves static frontend assets |
| Frontend | **Svelte 5** (plain, no SvelteKit) | SPA with lightweight router, talks to Axum API |
| JS runtime | **Bun** | For building/bundling the Svelte frontend |
| ORM | **SeaORM** | Async ORM for Rust with SQLite support, migrations, entity generation |
| Database | **SQLite** | Single file, via SeaORM's SQLite backend |
| Currency | **rusty-money** | Proper currency types instead of raw integer cents |
| File storage | **Local filesystem** | Structured directory: `{DATA_DIR}/files/{vehicle_id}/{record_type}/` |
| AI layer | **Pluggable trait** | Anthropic Claude API primary, OpenAI-compatible API (Ollama, LM Studio, OpenRouter, etc.) as fallback. Not needed for MVP |
| Deployment | **Native ARM64 binary** | Single compiled binary on Odroid N2+ running DietPi |
| Design | **Function-first** | Style/theming TBD. Focus on usability, not aesthetics, in early phases |

### Project structure (suggested)

```
glovebox/
├── Cargo.toml
├── src/                        # Rust backend
│   ├── main.rs                 # Axum server setup, static file serving
│   ├── entities/               # SeaORM entities (generated or hand-written)
│   │   ├── mod.rs
│   │   ├── platform.rs
│   │   ├── model_template.rs
│   │   ├── vehicle.rs
│   │   ├── vehicle_attribute.rs
│   │   ├── mileage_log.rs
│   │   ├── maintenance_schedule_item.rs
│   │   ├── service_record.rs
│   │   ├── service_schedule_link.rs
│   │   ├── shop.rs
│   │   ├── observation.rs
│   │   ├── part_slot.rs
│   │   ├── part.rs
│   │   ├── document.rs
│   │   ├── accident.rs
│   │   ├── accident_correspondence.rs
│   │   └── research_report.rs
│   ├── migration/              # SeaORM migrations
│   │   ├── lib.rs
│   │   ├── m20260301_000001_create_platforms.rs
│   │   ├── m20260301_000002_create_model_templates.rs
│   │   ├── m20260301_000003_create_vehicles.rs
│   │   └── ...                 # One file per migration
│   ├── api/                    # Axum route handlers
│   │   ├── mod.rs
│   │   ├── vehicles.rs
│   │   ├── maintenance.rs
│   │   ├── services.rs
│   │   ├── observations.rs
│   │   ├── parts.rs
│   │   ├── documents.rs
│   │   ├── accidents.rs
│   │   ├── shops.rs
│   │   ├── mileage.rs
│   │   └── research.rs
│   ├── services/               # Business logic
│   │   ├── mod.rs
│   │   ├── reminders.rs        # Reminder calculation engine
│   │   ├── mileage.rs          # Mileage extrapolation
│   │   ├── vin_decode.rs       # NHTSA VIN decoder client
│   │   ├── bundling.rs         # Service bundling suggestions
│   │   └── ai/                 # AI abstraction (post-MVP)
│   │       ├── mod.rs          # Trait definition
│   │       ├── claude.rs       # Anthropic API implementation
│   │       └── openai_compat.rs # OpenAI-compatible API (Ollama, LM Studio, etc.)
│   └── config.rs               # App configuration
├── frontend/                   # Svelte 5 SPA
│   ├── package.json
│   ├── bunfig.toml
│   ├── vite.config.ts
│   ├── src/
│   │   ├── main.ts
│   │   ├── App.svelte
│   │   ├── router.ts           # Lightweight client-side router
│   │   ├── lib/
│   │   │   ├── api.ts          # API client (typed fetch wrappers)
│   │   │   ├── types.ts        # TypeScript types matching Rust models
│   │   │   └── stores.ts       # Svelte stores (selected vehicle, etc.)
│   │   ├── components/
│   │   │   ├── Garage.svelte           # Landing page: car cards grid
│   │   │   ├── VehicleCard.svelte      # Car card with status summary
│   │   │   ├── VehicleDetail.svelte    # Tabbed detail view
│   │   │   ├── ScheduleTab.svelte      # Maintenance schedule view
│   │   │   ├── HistoryTab.svelte       # Service history timeline
│   │   │   ├── PartsTab.svelte         # Slots & installed parts (Phase 3)
│   │   │   ├── ObservationsTab.svelte  # Journal/logbook (Phase 2)
│   │   │   ├── DocumentsTab.svelte     # Attached files (Phase 2)
│   │   │   ├── MileageEntry.svelte     # Quick mileage check-in
│   │   │   ├── ServiceForm.svelte      # Log a completed service
│   │   │   ├── ScheduleItemForm.svelte # Add/edit a schedule item
│   │   │   └── VehicleSetup.svelte     # New car wizard (VIN decode flow)
│   │   └── styles/
│   │       └── global.css
│   └── dist/                   # Built output, served by Axum
└── data/                       # Runtime data (gitignored)
    ├── glovebox.db             # SQLite database
    └── files/                  # Uploaded documents
        ├── {vehicle_id}/
        │   ├── services/
        │   ├── parts/
        │   ├── observations/
        │   └── general/
        └── ...
```

---

## Data Model

### Design Principles

- **Currency:** All monetary values use `rusty-money` `Money` types in Rust. In SQLite, stored as integer cents with a currency code column (e.g., `cost_cents INTEGER`, `cost_currency TEXT DEFAULT 'USD'`). The Rust layer handles conversion.
- **ORM:** All entities are defined as SeaORM entities with derive macros. The SQL below is illustrative of the schema shape; actual migrations will be generated by SeaORM's migration tool.
- **Inheritance:** Maintenance schedules use a three-level hierarchy: Platform → Model Template → Vehicle. Vehicles inherit schedules from their model template, which inherits from its platform. Overrides at any level take precedence.

### Vehicle Hierarchy: Platforms, Model Templates & Vehicles

The key insight: many cars share maintenance fundamentals at the platform level (e.g., all MQB/EA888 cars need the same oil change interval), differ at the model level (GTI manual vs Alltrack DSG+Haldex), and get personalized at the vehicle level (Steve runs 5k oil intervals instead of 10k).

```
Platform: VW MQB (EA888)
  common schedules: oil, coolant, serpentine belt, cabin filter, air filter, brake fluid, wipers

  Model Template: 2017 Golf GTI Mk7 (6MT, FWD)
    adds: manual transmission fluid, spark plugs at 60k
    removes: Haldex service, DSG service (not applicable)
    → Steve's GTI (55k mi, modded)
       overrides: oil interval 10k→5k

  Model Template: 2017 Golf Alltrack Mk7 (DSG, 4MOTION)
    adds: Haldex service, DSG service, timing chain tensioner check
    → Steve's Alltrack (115k mi, stock)
       overrides: oil interval 10k→5k
```

#### `platforms`
```sql
CREATE TABLE platforms (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,              -- e.g., "VW MQB (EA888)"
    manufacturer TEXT NOT NULL,      -- e.g., "Volkswagen"
    description TEXT,                -- e.g., "Modular Transverse Matrix platform, 2.0L EA888 turbo inline-4"
    years TEXT,                      -- e.g., "2015-2021" (informational)
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

#### `model_templates`
```sql
CREATE TABLE model_templates (
    id INTEGER PRIMARY KEY,
    platform_id INTEGER REFERENCES platforms(id),  -- NULL if no platform grouping
    name TEXT NOT NULL,              -- e.g., "2017 Golf GTI Mk7 (6MT, FWD)"
    year INTEGER,
    make TEXT,
    model TEXT,
    trim TEXT,
    engine TEXT,                     -- e.g., "2.0L TSI (EA888 Gen3)"
    transmission TEXT,               -- e.g., "6MT", "DSG (DQ250)"
    drivetrain TEXT,                 -- e.g., "FWD", "4MOTION"
    source TEXT,                     -- "manual", "ai_extracted", "community"
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

#### `vehicles`
```sql
CREATE TABLE vehicles (
    id INTEGER PRIMARY KEY,
    model_template_id INTEGER REFERENCES model_templates(id),  -- Links to inherited schedules
    vin TEXT UNIQUE,
    year INTEGER NOT NULL,
    make TEXT NOT NULL,
    model TEXT NOT NULL,
    trim TEXT,
    engine TEXT,
    transmission TEXT,
    drivetrain TEXT,
    exterior_color TEXT,
    interior_color TEXT,
    purchase_date TEXT,              -- ISO date
    purchase_price_cents INTEGER,
    purchase_price_currency TEXT DEFAULT 'USD',
    purchase_odometer INTEGER,
    dealership TEXT,
    loan_provider TEXT,
    loan_amount_cents INTEGER,
    loan_amount_currency TEXT DEFAULT 'USD',
    loan_apr REAL,
    sale_date TEXT,                  -- NULL if still owned
    sale_price_cents INTEGER,
    sale_price_currency TEXT DEFAULT 'USD',
    sale_odometer INTEGER,
    status TEXT NOT NULL DEFAULT 'owned',  -- 'owned', 'sold'
    photo_path TEXT,
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

#### `vehicle_attributes`
```sql
-- Flexible key-value attributes for a vehicle. Replaces the monolithic
-- vin_decoded_data JSON blob. Queryable, auditable, multi-sourced.
CREATE TABLE vehicle_attributes (
    id INTEGER PRIMARY KEY,
    vehicle_id INTEGER NOT NULL REFERENCES vehicles(id),
    key TEXT NOT NULL,               -- e.g., "engine_displacement", "tire_size_front", "curb_weight"
    value TEXT NOT NULL,             -- Always stored as text, interpreted by the application
    unit TEXT,                       -- e.g., "L", "lbs", "mm" (optional)
    source TEXT NOT NULL DEFAULT 'manual',  -- 'vin_decode', 'manual', 'ai_extracted', 'obd'
    source_detail TEXT,              -- e.g., NHTSA field name, or "owner's manual page 42"
    superseded_by INTEGER REFERENCES vehicle_attributes(id),  -- For tracking changes over time
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(vehicle_id, key, source)  -- One value per key per source; update to supersede
);

CREATE INDEX idx_vehicle_attrs ON vehicle_attributes(vehicle_id, key);
```

This approach lets you:
- Store NHTSA-decoded data as individual keyed attributes (source = 'vin_decode')
- Manually add/override attributes (source = 'manual')
- Have AI extract attributes from documents (source = 'ai_extracted')
- Query attributes directly: "show me all vehicles with engine_displacement = '2.0'"
- Track attribute history via the `superseded_by` chain

#### `mileage_log`
```sql
CREATE TABLE mileage_log (
    id INTEGER PRIMARY KEY,
    vehicle_id INTEGER NOT NULL REFERENCES vehicles(id),
    odometer INTEGER NOT NULL,
    recorded_at TEXT NOT NULL DEFAULT (datetime('now')),
    source TEXT NOT NULL DEFAULT 'manual',  -- 'manual', 'service', 'observation', 'obd'
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Index for mileage projection calculations
CREATE INDEX idx_mileage_log_vehicle_date ON mileage_log(vehicle_id, recorded_at);
```

#### `maintenance_schedule_items`

Schedule items can exist at three levels: platform, model template, or vehicle. The inheritance chain is:

**Platform items** → apply to all vehicles on that platform (e.g., "Oil Change every 10k mi" for all MQB/EA888)
**Model template items** → add to or override platform items for a specific model (e.g., "Haldex Service every 40k" for Alltrack only)
**Vehicle items** → add to or override inherited items for a specific car (e.g., Steve's GTI overrides oil to 5k)

An item at a lower level with the same `name` as a higher-level item is treated as an **override**. If the vehicle-level item sets `enabled = FALSE`, it suppresses the inherited item entirely (useful for "DSG Service doesn't apply to my manual GTI").

```sql
CREATE TABLE maintenance_schedule_items (
    id INTEGER PRIMARY KEY,
    -- Exactly one of these three should be set (the "owner" of this schedule item)
    platform_id INTEGER REFERENCES platforms(id),
    model_template_id INTEGER REFERENCES model_templates(id),
    vehicle_id INTEGER REFERENCES vehicles(id),
    -- If this item overrides a parent item, link to it
    overrides_item_id INTEGER REFERENCES maintenance_schedule_items(id),
    name TEXT NOT NULL,                    -- e.g., "Oil Change", "DSG Service"
    description TEXT,
    interval_miles INTEGER,                -- NULL if time-only
    interval_months INTEGER,               -- NULL if mileage-only
    warning_miles INTEGER DEFAULT 1000,    -- Remind this many miles before due
    warning_days INTEGER DEFAULT 30,       -- Remind this many days before due
    labor_categories TEXT,                 -- JSON array: ["undercar", "engine_bay"] for bundling
    is_factory_recommended BOOLEAN DEFAULT FALSE,
    source TEXT,                           -- "factory", "community", "custom"
    notes TEXT,
    enabled BOOLEAN NOT NULL DEFAULT TRUE, -- FALSE = suppress this item (useful for overrides)
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    -- Constraint: exactly one owner
    CHECK (
        (platform_id IS NOT NULL AND model_template_id IS NULL AND vehicle_id IS NULL) OR
        (platform_id IS NULL AND model_template_id IS NOT NULL AND vehicle_id IS NULL) OR
        (platform_id IS NULL AND model_template_id IS NULL AND vehicle_id IS NOT NULL)
    )
);

CREATE INDEX idx_schedule_platform ON maintenance_schedule_items(platform_id) WHERE platform_id IS NOT NULL;
CREATE INDEX idx_schedule_template ON maintenance_schedule_items(model_template_id) WHERE model_template_id IS NOT NULL;
CREATE INDEX idx_schedule_vehicle ON maintenance_schedule_items(vehicle_id) WHERE vehicle_id IS NOT NULL;
```

**Resolution algorithm** (implemented in Rust `services/reminders.rs`):
1. Start with the vehicle's platform items (if any)
2. Layer on model template items — items with same `name` override the platform version
3. Layer on vehicle items — items with same `name` override the model template version
4. Filter out items where `enabled = FALSE`
5. Result: the effective maintenance schedule for this specific vehicle

#### `shops`
```sql
CREATE TABLE shops (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    address TEXT,
    phone TEXT,
    website TEXT,
    specialty TEXT,             -- e.g., "VW/Audi specialist", "General", "Tires"
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

#### `service_records`
```sql
CREATE TABLE service_records (
    id INTEGER PRIMARY KEY,
    vehicle_id INTEGER NOT NULL REFERENCES vehicles(id),
    shop_id INTEGER REFERENCES shops(id),
    title TEXT NOT NULL,                   -- e.g., "60k Service", "Oil Change"
    description TEXT,
    odometer INTEGER NOT NULL,             -- Required: critical for reminder calculations
    service_date TEXT NOT NULL,            -- Required: the primary date of the service (ISO date)
    -- Optional shop-specific dates (Phase 6: service-in-progress tracking)
    drop_off_date TEXT,
    pick_up_date TEXT,
    total_cost_cents INTEGER,
    total_cost_currency TEXT DEFAULT 'USD',
    labor_cost_cents INTEGER,
    labor_cost_currency TEXT DEFAULT 'USD',
    parts_cost_cents INTEGER,
    parts_cost_currency TEXT DEFAULT 'USD',
    is_diy BOOLEAN DEFAULT FALSE,
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Many-to-many: which schedule items does this service satisfy?
CREATE TABLE service_schedule_links (
    service_record_id INTEGER NOT NULL REFERENCES service_records(id),
    schedule_item_id INTEGER NOT NULL REFERENCES maintenance_schedule_items(id),
    PRIMARY KEY (service_record_id, schedule_item_id)
);
```

#### `observations`
*(Phase 2, but define the table now for forward compatibility)*
```sql
CREATE TABLE observations (
    id INTEGER PRIMARY KEY,
    vehicle_id INTEGER NOT NULL REFERENCES vehicles(id),
    category TEXT NOT NULL,      -- 'noise', 'warning_light', 'cosmetic', 'performance', 'obd_code', 'general'
    title TEXT NOT NULL,
    description TEXT,
    odometer INTEGER,
    observed_at TEXT NOT NULL DEFAULT (datetime('now')),
    obd_codes TEXT,              -- JSON array of codes: ["P0301", "P0302"]
    resolved BOOLEAN DEFAULT FALSE,
    resolved_service_id INTEGER REFERENCES service_records(id),
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

#### `parts`
*(Phase 3, but define schema now)*
```sql
-- Slots define "positions" on a vehicle (e.g., "Diverter Valve", "Front Tires")
CREATE TABLE part_slots (
    id INTEGER PRIMARY KEY,
    vehicle_id INTEGER NOT NULL REFERENCES vehicles(id),
    name TEXT NOT NULL,                  -- e.g., "Diverter Valve", "Front Tires"
    category TEXT,                       -- e.g., "engine", "suspension", "wheels_tires", "brakes"
    oe_spec TEXT,                        -- OE part description: "18\" 245/40R18"
    oe_part_number TEXT,                 -- Factory part number
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Individual parts (purchased, installed, or replaced)
CREATE TABLE parts (
    id INTEGER PRIMARY KEY,
    slot_id INTEGER REFERENCES part_slots(id),
    vehicle_id INTEGER NOT NULL REFERENCES vehicles(id),
    name TEXT NOT NULL,                  -- "GFB DV+", "Toyo Proxes 4S 245/40R18"
    manufacturer TEXT,
    part_number TEXT,                    -- Manufacturer's part number
    oe_part_number_replaced TEXT,        -- Which OE part this replaces
    seller TEXT,                         -- "Tire Rack", "ECS Tuning"
    purchase_date TEXT,
    cost_cents INTEGER,
    cost_currency TEXT DEFAULT 'USD',
    invoice_url TEXT,                    -- Could be a document reference
    status TEXT NOT NULL DEFAULT 'purchased',  -- 'purchased', 'installed', 'replaced', 'returned'
    installed_date TEXT,
    installed_odometer INTEGER,
    installed_service_id INTEGER REFERENCES service_records(id),
    replaced_date TEXT,
    replaced_odometer INTEGER,
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

#### `documents`
```sql
CREATE TABLE documents (
    id INTEGER PRIMARY KEY,
    vehicle_id INTEGER REFERENCES vehicles(id),  -- NULL for non-vehicle docs
    title TEXT NOT NULL,
    file_path TEXT NOT NULL,               -- Relative path within data/files/
    file_name TEXT NOT NULL,               -- Original filename
    mime_type TEXT,
    file_size_bytes INTEGER,
    doc_type TEXT,                          -- 'invoice', 'receipt', 'photo', 'title', 'warranty', 'manual', 'other'
    -- Polymorphic link: a document can belong to any entity
    linked_entity_type TEXT,               -- 'service_record', 'part', 'observation', 'accident', 'vehicle'
    linked_entity_id INTEGER,
    notes TEXT,
    -- AI-extracted text (Phase 4)
    extracted_text TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

#### `accidents`
```sql
CREATE TABLE accidents (
    id INTEGER PRIMARY KEY,
    vehicle_id INTEGER NOT NULL REFERENCES vehicles(id),
    occurred_at TEXT NOT NULL,
    odometer INTEGER,
    description TEXT NOT NULL,
    fault TEXT,                    -- 'mine', 'theirs', 'shared', 'unknown'
    -- Other party info
    other_party_name TEXT,
    other_party_phone TEXT,
    other_party_email TEXT,
    other_party_insurance TEXT,
    other_party_policy_number TEXT,
    -- Our insurance
    insurance_claim_number TEXT,
    insurance_adjuster TEXT,
    insurance_adjuster_phone TEXT,
    -- Outcome
    total_repair_cost_cents INTEGER,
    total_repair_cost_currency TEXT DEFAULT 'USD',
    deductible_cents INTEGER,
    deductible_currency TEXT DEFAULT 'USD',
    insurance_payout_cents INTEGER,
    insurance_payout_currency TEXT DEFAULT 'USD',
    resolved BOOLEAN DEFAULT FALSE,
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Correspondence log for insurance/accident communication
CREATE TABLE accident_correspondence (
    id INTEGER PRIMARY KEY,
    accident_id INTEGER NOT NULL REFERENCES accidents(id),
    occurred_at TEXT NOT NULL,
    contact_method TEXT,          -- 'phone', 'email', 'in_person', 'mail'
    contact_with TEXT,            -- Who you talked to
    summary TEXT NOT NULL,
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Link accident to its repair service records
CREATE TABLE accident_service_links (
    accident_id INTEGER NOT NULL REFERENCES accidents(id),
    service_record_id INTEGER NOT NULL REFERENCES service_records(id),
    PRIMARY KEY (accident_id, service_record_id)
);
```

#### `research_reports`
*(Phase 5, but worth knowing the shape)*
```sql
CREATE TABLE research_reports (
    id INTEGER PRIMARY KEY,
    vehicle_id INTEGER NOT NULL REFERENCES vehicles(id),
    generated_at TEXT NOT NULL DEFAULT (datetime('now')),
    report_type TEXT,             -- 'full_check', 'recalls_only', 'community_wisdom'
    summary TEXT,
    raw_data TEXT,                -- JSON blob of all findings
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE research_findings (
    id INTEGER PRIMARY KEY,
    report_id INTEGER NOT NULL REFERENCES research_reports(id),
    category TEXT NOT NULL,       -- 'recall', 'forum_report', 'suggested_maintenance', 'upgrade_idea'
    title TEXT NOT NULL,
    description TEXT,
    source_url TEXT,
    severity TEXT,                -- 'critical', 'recommended', 'optional', 'informational'
    status TEXT NOT NULL DEFAULT 'new',  -- 'new', 'dismissed', 'planned', 'completed'
    -- Link to action taken
    linked_entity_type TEXT,      -- 'service_record', 'part', 'maintenance_schedule'
    linked_entity_id INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

---

## Reminder Engine (Core MVP Logic)

The reminder engine is the most important business logic in Phase 1. Here's how it works:

### Inputs
- `maintenance_schedules` for the vehicle (what's due and at what intervals)
- `service_records` linked to each schedule item (when it was last done)
- `mileage_log` entries (current and historical mileage)

### Calculations

1. **Current mileage estimate:** Take the most recent `mileage_log` entry. If it's more than a day old, extrapolate using the vehicle's average daily mileage (calculated from the last N mileage entries, weighted toward recent).

2. **For each enabled schedule item:**
   - Find the most recent `service_record` that links to this schedule item
   - If no record exists, use the vehicle's `purchase_odometer` and `purchase_date` as the baseline (assumes it was done at purchase)
   - Calculate **miles until due:** `(last_service_odometer + interval_miles) - current_estimated_mileage`
   - Calculate **days until due:** `(last_service_date + interval_months) - today`
   - The item is due at **whichever comes first** (if both interval types are set)
   - Apply warning thresholds to determine status

3. **Status classification:**
   - `overdue` — past due by either mileage or time
   - `due_now` — within 0 miles/days of the threshold
   - `upcoming` — within the warning window (per-item `warning_miles` / `warning_days`)
   - `ok` — not yet within the warning window

4. **Bundling suggestions:** When generating the reminder list, group items by `labor_categories`. If an item is `due_now` or `overdue`, and another item in the same labor category is `upcoming` or will be due within a configurable window (e.g., next 5,000 miles), suggest doing them together.

### API Shape

```
GET /api/vehicles/{id}/reminders
```

Returns:
```json
{
  "vehicle_id": 1,
  "estimated_mileage": 55560,
  "mileage_as_of": "2026-03-06T12:00:00Z",
  "avg_daily_miles": 20.3,
  "reminders": [
    {
      "schedule_item": { "id": 3, "name": "Oil Change" },
      "status": "upcoming",
      "last_service": { "id": 12, "date": "2025-11-15", "odometer": 52000 },
      "due_at_miles": 57000,
      "due_at_date": "2026-05-15",
      "miles_remaining": 1440,
      "days_remaining": 70,
      "trigger": "mileage"
    }
  ],
  "bundle_suggestions": [
    {
      "reason": "These items share 'undercar' labor — consider doing them in the same visit",
      "items": [
        { "id": 3, "name": "Oil Change", "status": "upcoming" },
        { "id": 7, "name": "Transmission Fluid", "status": "ok", "due_in_miles": 4500 }
      ]
    }
  ]
}
```

---

## VIN Decode Integration

Use the **NHTSA vPIC API** (free, no auth required):

```
GET https://vpic.nhtsa.dot.gov/api/vehicles/DecodeVinValues/{VIN}?format=json
```

Returns ~140 fields including year, make, model, trim, engine displacement, engine configuration, fuel type, drive type, body class, etc. Key fields (year, make, model, trim, engine, transmission, drivetrain) are extracted into the `vehicles` table columns. All decoded fields are also stored as individual rows in `vehicle_attributes` with `source = 'vin_decode'`, making them queryable and auditable.

### NHTSA Recall API (Phase 5)

```
GET https://api.nhtsa.gov/recalls/recallsByVehicle?make=Volkswagen&model=Golf&modelYear=2017
```

Free, no auth. Returns active recalls for the vehicle. Can be checked periodically or on-demand via the "check all things" button.

---

## UX Flow

### Garage (Landing Page)
```
┌─────────────────────────────────────────────────┐
│  GLOVEBOX                          [+ Add Car]  │
├─────────────────────────────────────────────────┤
│                                                  │
│  ┌──────────────────┐  ┌──────────────────┐     │
│  │  [photo]         │  │  [photo]         │     │
│  │  2017 Golf GTI   │  │  2017 Alltrack   │     │
│  │  ~55,560 mi      │  │  ~115,200 mi     │     │
│  │                  │  │                  │     │
│  │  ● 1 upcoming    │  │  ● 2 overdue     │     │
│  │  ○ All good      │  │  ● 3 upcoming    │     │
│  └──────────────────┘  └──────────────────┘     │
│                                                  │
└─────────────────────────────────────────────────┘
```

### Vehicle Detail (after clicking a car)
```
┌─────────────────────────────────────────────────┐
│  ← Garage    2017 Golf GTI SE    [Edit] [⚙]    │
├─────────────────────────────────────────────────┤
│  55,560 mi (est.)   Last updated: 3 days ago    │
│  [Update Mileage]   [Log Service]               │
├─────────────────────────────────────────────────┤
│  [Schedule] [History] [Parts] [Obs.] [Docs]     │
├─────────────────────────────────────────────────┤
│                                                  │
│  SCHEDULE TAB:                                   │
│                                                  │
│  ⚠ OVERDUE                                      │
│  (none)                                          │
│                                                  │
│  ● UPCOMING                                      │
│  ┌────────────────────────────────────────┐     │
│  │ Oil Change                              │     │
│  │ Due at 57,000 mi or May 15, 2026       │     │
│  │ ~1,440 miles / 70 days remaining       │     │
│  │ Last done: Nov 15, 2025 @ 52,000 mi   │     │
│  │ [Log as Done]  [Snooze]  [Edit]        │     │
│  └────────────────────────────────────────┘     │
│                                                  │
│  💡 BUNDLE SUGGESTION                            │
│  "Oil Change is coming up. While under the car, │
│   consider doing Transmission Fluid too          │
│   (due in ~4,500 mi)"                           │
│                                                  │
│  ✓ OK (not yet due)                              │
│  ┌────────────────────────────────────────┐     │
│  │ Spark Plugs — due at 60,000 mi         │     │
│  │ Manual Trans Fluid — due at 60,000 mi  │     │
│  │ Brake Fluid — due at Jun 2026          │     │
│  │ ...                                     │     │
│  └────────────────────────────────────────┘     │
│                                                  │
└─────────────────────────────────────────────────┘
```

### New Car Setup Wizard
1. **Enter VIN** → Call NHTSA decode → Auto-fill year/make/model/trim/engine
2. **Confirm & augment** → Edit decoded info, add purchase date/price/loan info, upload photo
3. **Seed schedule** → Present factory-recommended schedule items (pre-populated if available). User can adjust intervals, enable/disable items, add custom items
4. **Initial state** → For each schedule item, enter: "Last done at ___ miles on ___" (or "never / unknown"). Enter current odometer reading
5. **Done** → Car appears in garage with reminders calculated

---

## Phase Plan

### Phase 1 — MVP: "Stop Missing Oil Changes"
**Goal:** Working reminder system for two cars. Use it weekly.

**Backend:**
- Vehicle CRUD (create, read, update; no delete needed yet)
- VIN decode via NHTSA API on vehicle creation
- Maintenance schedule CRUD (per vehicle)
- Service record CRUD (with links to schedule items)
- Mileage log entries (manual)
- Reminder engine (calculation + bundling logic)
- API endpoints for all of the above
- SQLite migrations
- Static file serving for the Svelte SPA

**Frontend:**
- Garage landing page (vehicle cards with status)
- Vehicle detail page with Schedule tab and History tab
- New car setup wizard (VIN decode flow)
- Mileage entry (quick form — just odometer + optional note)
- Service record form (date, shop name as free text for now, cost, which schedule items it covers, notes)
- Schedule item form (name, intervals, warning thresholds)
- Reminder dashboard (overdue / upcoming / ok)
- Bundle suggestions on the schedule view

**Data seeding:**
- Create the "VW MQB (EA888)" platform with shared schedule items
- Create model templates for "2017 Golf GTI Mk7 (6MT, FWD)" and "2017 Golf Alltrack Mk7 (DSG, 4MOTION)" with model-specific schedule items
- During car setup wizard, link the vehicle to its model template and allow vehicle-level interval overrides
- If building the full hierarchy is too much for MVP, flatten it: just create vehicle-level schedule items with common presets (oil, spark plugs, air filter, cabin filter, brake fluid, transmission fluid, coolant, tires, brakes, wipers)

**Not in Phase 1:** Document uploads, observations, parts tracking, AI features, shops as a separate entity (just free-text shop name on service records).

### Phase 2 — Documents & Observations
**Goal:** Turn it into a real ownership record.

- Document upload and attachment to any entity (service records, vehicle, etc.)
- File storage in structured local filesystem
- Observation/journal entries with categories and OBD code capture
- Link observations to service records when resolved
- Shops as a first-class entity (name, address, phone, website) with autocomplete on service records
- History tab improvements: timeline view showing services + observations interleaved

### Phase 3 — Parts & Cost Tracking
**Goal:** Know what's on your car and what it costs to own.

- Part slots per vehicle (OE spec + current installed part)
- Part purchase tracking (seller, cost, part numbers)
- Link parts to service records (installed during this service)
- Part history per slot (what was installed and when)
- Cost of ownership calculations: total spend, spend by category, spend over time
- Export service history (for selling the car — filtered, printable view)

### Phase 4 — AI Layer
**Goal:** Make the app smarter with AI assistance.

- Pluggable AI trait: `trait AiProvider { async fn complete(&self, prompt: &str, context: &str) -> Result<String>; }`
- Claude API implementation (primary)
- OpenAI-compatible API implementation (Ollama, LM Studio, OpenRouter, etc.)
- **Invoice parsing:** Upload a PDF → AI extracts date, shop, mileage, line items, costs → Pre-fills a service record form for review
- **Chat interface:** Natural language queries against your data ("when did I last change tires?", "how much have I spent on the GTI this year?")
- **Vehicle attribute population:** Feed owner's manual PDF → AI extracts maintenance schedule, specs, capacities
- **Proactive suggestions:** Based on current mileage, mod list, and service history, suggest upcoming maintenance or things to watch for

### Phase 5 — External Integrations & Research
**Goal:** Connect to the outside world.

- **NHTSA recall checking** by VIN (free API)
- **Research reports:** The "go check all the things" button — queries recalls, and presents findings as a report with actionable items
- Community wisdom integration (forum scraping or AI-summarized forum knowledge)
- Research findings linkable to actions (service records, parts purchases, schedule additions)
- OBD data import (if OBDEleven exports data in a parseable format)

### Phase 6 — Financier Integration & Multi-User
**Goal:** Connect the financial picture and prepare for family use.

- Financier integration: sync cost data, loan tracking, depreciation
- Multi-user support: user accounts, per-user car assignments
- Shared household view (all family cars)
- Notification system (email reminders for upcoming/overdue items — requires SMTP or similar)
- Service-in-progress status tracking ("GTI: at Clovis VW since 3/4")

---

## Key Rust Crates (Suggested)

| Crate | Purpose |
|-------|---------|
| `axum` | HTTP framework |
| `tokio` | Async runtime |
| `sea-orm` (with `sqlx-sqlite`, `runtime-tokio-rustls`) | Async ORM with migrations, entity generation, query builder |
| `sea-orm-migration` | Database migration framework |
| `serde` / `serde_json` | Serialization |
| `rusty-money` | Currency and money types with proper arithmetic |
| `reqwest` | HTTP client (for NHTSA API, AI APIs) |
| `chrono` | Date/time handling |
| `tower-http` | Middleware (CORS, static file serving, compression) |
| `tracing` / `tracing-subscriber` | Structured logging |
| `thiserror` / `anyhow` | Error handling |
| `clap` | CLI arg parsing (for config overrides, data dir path) |
| `uuid` | IDs (if preferred over integer PKs) |

## Frontend Routing

Lightweight client-side router (something like `svelte-spa-router` or a tiny hand-rolled one):

```
/                           → Garage (landing page)
/vehicles/new               → New car setup wizard
/vehicles/:id               → Vehicle detail (Schedule tab default)
/vehicles/:id/schedule      → Schedule tab
/vehicles/:id/history       → Service history tab
/vehicles/:id/parts         → Parts/slots tab (Phase 3)
/vehicles/:id/observations  → Observations tab (Phase 2)
/vehicles/:id/documents     → Documents tab (Phase 2)
/vehicles/:id/services/new  → Log a new service
/vehicles/:id/services/:id  → Service record detail
/vehicles/:id/accidents     → Accident records (Phase 2+)
/shops                      → Shop directory (Phase 2)
/settings                   → App settings (intervals defaults, AI config, etc.)
```

---

## Configuration

Configuration is split into two tiers:

### Bootstrap config (CLI args / env vars via `clap`)

Only the bare minimum needed to start the server. Sensible defaults mean these rarely need to be set.

```
GLOVEBOX_DB_PATH=./data/glovebox.db      # or --db-path
GLOVEBOX_LISTEN=0.0.0.0:3003             # or --listen
GLOVEBOX_FILES_DIR=./data/files           # or --files-dir
```

### Application config (in the database)

Everything else lives in a `settings` table, editable through the Settings page in the UI. Backed up automatically with the DB.

```sql
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,          -- Stored as text, parsed by the application
    description TEXT,             -- Human-readable explanation for the Settings UI
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

**Default settings (seeded on first run):**

| Key | Default | Description |
|-----|---------|-------------|
| `reminders.default_warning_miles` | `1000` | Default miles-before-due reminder threshold |
| `reminders.default_warning_days` | `30` | Default days-before-due reminder threshold |
| `reminders.mileage_extrapolation_lookback_days` | `90` | How many days of mileage history to use for extrapolation |
| `reminders.bundling_window_miles` | `5000` | Suggest bundling items due within this many miles of each other |
| `ai.provider` | `none` | AI provider: `none`, `claude`, `openai_compat` |
| `ai.claude_api_key` | *(empty)* | Anthropic API key (Phase 4) |
| `ai.claude_model` | `claude-sonnet-4-6` | Claude model to use |
| `ai.openai_api_base` | `http://localhost:11434/v1` | OpenAI-compatible API base URL (works with Ollama, LM Studio, OpenRouter, etc.) |
| `ai.openai_api_key` | *(empty)* | API key (optional — not needed for local providers like Ollama) |
| `ai.openai_model` | `llama3` | Model name for OpenAI-compatible provider |

---

## Initial Maintenance Schedule Template (2017 VW MQB / EA888)

Pre-populated schedule items to seed during car setup. User adjusts to their preference.

### Platform level: VW MQB (EA888)
*Shared by both GTI and Alltrack (and any future MQB car)*

| Item | Miles | Months | Labor Categories | Notes |
|------|-------|--------|-----------------|-------|
| Oil & Filter Change | 10,000 | 12 | `["undercar"]` | Factory interval; expect vehicle-level override to shorter |
| Spark Plugs | 60,000 | — | `["engine_bay"]` | |
| Air Filter | 20,000 | 24 | `["engine_bay"]` | |
| Cabin Air Filter | 20,000 | 24 | `["interior"]` | Behind glove box |
| Brake Fluid Flush | — | 24 | `["brakes"]` | VW recommends every 2 years |
| Coolant | 100,000 | — | `["engine_bay"]` | |
| Serpentine Belt | 80,000 | — | `["engine_bay"]` | Inspect annually after 60k |
| Tire Rotation | 5,000 | 6 | `["wheels_off"]` | |
| Brake Pads & Rotors | Inspect | 12 | `["wheels_off", "brakes"]` | Wear item, no fixed mileage interval |
| Wiper Blades | — | 12 | `["exterior"]` | |

### Model template level: 2017 Golf GTI Mk7 (6MT, FWD)
*Adds manual-transmission-specific items*

| Item | Miles | Months | Labor Categories | Notes |
|------|-------|--------|-----------------|-------|
| Manual Transmission Fluid | 60,000 | — | `["undercar"]` | Not in factory schedule but widely recommended |
| Carbon Cleaning (walnut blast) | 80,000 | — | `["engine_bay"]` | Direct injection buildup |
| Timing Chain Tensioner | Inspect | — | `["engine_bay"]` | Known EA888 weak point; inspect at 80k+ |
| Water Pump | — | — | `["engine_bay"]` | Known failure around 80-100k, inspect proactively |

### Model template level: 2017 Golf Alltrack Mk7 (DSG, 4MOTION)
*Adds DSG and Haldex AWD items*

| Item | Miles | Months | Labor Categories | Notes |
|------|-------|--------|-----------------|-------|
| DSG Fluid & Filter | 40,000 | — | `["undercar"]` | Critical for DSG longevity |
| Haldex Fluid & Filter | 40,000 | — | `["undercar"]` | Critical for AWD system |
| Carbon Cleaning (walnut blast) | 80,000 | — | `["engine_bay"]` | Direct injection buildup |
| Timing Chain Tensioner | Inspect | — | `["engine_bay"]` | Known EA888 weak point, especially at high mileage |
| Water Pump | — | — | `["engine_bay"]` | Known failure around 80-100k, inspect proactively |

### Vehicle level overrides: Steve's GTI & Alltrack
*Both cars override oil interval to 5k/6mo due to preference for turbo engines*

| Item | Override | Notes |
|------|----------|-------|
| Oil & Filter Change | 5,000 mi / 6 months | Shorter than factory for turbo longevity |

*Note: The schedule template data above demonstrates the three-level inheritance. In the database, each row in `maintenance_schedule_items` has exactly one of `platform_id`, `model_template_id`, or `vehicle_id` set, plus the `overrides_item_id` link when overriding a parent item.*

---

## Open Questions & Future Ideas

- **Shared Rust crate with Homie?** Both apps deal with entities, events, documents, maintenance schedules, and relationships. A shared `entity-core` crate could provide the generic patterns, with Glovebox and Homie adding domain-specific layers on top. Worth exploring after both apps are more mature.
- **Mobile access:** Since it's a web app on the Odroid, it's accessible from a phone browser on the local network. For remote access (at the mechanic), Tailscale or similar. A PWA manifest would let it feel more app-like on mobile.
- **Backup strategy:** SQLite file + the files directory. A cron job running `sqlite3 glovebox.db ".backup /backup/glovebox-$(date +%Y%m%d).db"` plus rsync for the files dir.
- **VW-specific maintenance schedule sourcing:** The 2017 VW owner's manual PDFs are available from VW's website. The AI layer could eventually parse these to auto-generate schedule templates for any VW model/year. For MVP, the hardcoded templates above are a good start.
- **OBDEleven data export:** OBDEleven stores scan data in their cloud. They may have an API or export feature worth investigating for Phase 5.
- **Cost of ownership comparison:** When you eventually have data for both cars, it'd be interesting to compare $/mile, $/month, category breakdowns side by side.
