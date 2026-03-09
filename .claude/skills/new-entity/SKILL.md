---
name: new-entity
description: |
  Scaffold a full-stack CRUD entity across Rust backend and Svelte frontend.
  Use when adding a new resource to the glovebox app — creates entity, handler,
  migration, TypeScript types, API client, and route wiring.
  Trigger on: "new entity", "add resource", "scaffold", "new table".
user_invocable: true
---

# Full-Stack Entity Scaffold

When the user asks to add a new entity/resource, follow this checklist to create all required files. Ask the user for:

1. **Entity name** (singular, snake_case for Rust, e.g. `warranty`)
2. **Fields** (name, type, nullable?) — beyond the standard `id`, `created_at`, `updated_at`
3. **Parent resource**: Is this a **top-level** resource or a **vehicle sub-resource**?
4. **Relations**: Any foreign keys to existing entities?
5. **Junction tables**: Any many-to-many relationships needed?

---

## Files to Create/Modify (in order)

### 1. Entity — `src/entities/{name}.rs`

```rust
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "{plural_name}")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    // --- user fields here ---
    // For vehicle sub-resources: pub vehicle_id: i32,
    // Nullable fields: Option<T>
    // Currency: Option<i32> cents + Option<String> currency
    // Dates: String (SQLite TEXT)
    pub created_at: String,
    pub updated_at: String,
    // Fields added by ALTER TABLE go AFTER updated_at
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    // belongs_to for FK references:
    // #[sea_orm(
    //     belongs_to = "super::vehicle::Entity",
    //     from = "Column::VehicleId",
    //     to = "super::vehicle::Column::Id"
    // )]
    // Vehicle,
}

// Related impls for each relation variant
// impl Related<super::vehicle::Entity> for Entity {
//     fn to() -> RelationDef { Relation::Vehicle.def() }
// }

impl ActiveModelBehavior for ActiveModel {}
```

**Rules:**
- Field order MUST match physical DB column order (ALTER TABLE appends to end)
- Use `sea_orm::entity::prelude::*` import
- DateTime columns are `String` (SQLite TEXT), never chrono types
- Currency: pair of `Option<i32>` (cents) + `Option<String>` (currency code)
- Junction tables: composite PK with `#[sea_orm(primary_key, auto_increment = false)]` on both fields, no `id`
- Parent entities declare `has_many`; junction tables need `Related` + `via()` on both sides

### 2. Register Entity — `src/entities/mod.rs`

Add `pub mod {name};` in alphabetical order.

### 3. API Handler — `src/api/{name}.rs` (or `{plural_name}.rs`)

**Top-level resource pattern** (like shops):
```rust
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use sea_orm::*;
use serde::Deserialize;

use crate::entities::{name};
use crate::AppState;
use super::error::ApiError;

type Result<T> = std::result::Result<T, ApiError>;

#[derive(Deserialize)]
pub struct Create{Name} {
    // Required fields (bare types)
    // Optional fields (Option<T>)
}

#[derive(Deserialize)]
pub struct Update{Name} {
    // Required fields: Option<T> (present = change, absent = keep)
    // Nullable fields: Option<Option<T>> (None = keep, Some(None) = clear, Some(val) = set)
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<{name}::Model>>> {
    let items = {name}::Entity::find()
        .order_by_asc({name}::Column::Name)  // or appropriate sort
        .all(&state.db)
        .await?;
    Ok(Json(items))
}

async fn get_one(State(state): State<AppState>, Path(id): Path<i32>) -> Result<Json<{name}::Model>> {
    {name}::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("{Name} {id} not found")))
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<Create{Name}>,
) -> Result<Json<{name}::Model>> {
    let model = {name}::ActiveModel {
        // field: Set(input.field),
        ..Default::default()
    };
    let result = model.insert(&state.db).await?;
    Ok(Json(result))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(input): Json<Update{Name}>,
) -> Result<Json<{name}::Model>> {
    let existing = {name}::Entity::find_by_id(id)
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("{Name} {id} not found")))?;

    let mut active: {name}::ActiveModel = existing.into();

    // if let Some(v) = input.field { active.field = Set(v); }

    active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());

    let result = active.update(&state.db).await?;
    Ok(Json(result))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(get_one).put(update))
}
```

**Vehicle sub-resource pattern** (like observations, services):
- NO `router()` function — uses flat routes in main.rs
- `list()` takes `Path(vehicle_id): Path<i32>`, calls `require_vehicle()` first
- `get_one()` takes `Path((vehicle_id, id)): Path<(i32, i32)>`
- `create()` takes `Path(vehicle_id): Path<i32>`, sets `vehicle_id: Set(vehicle_id)`
- `update()` takes `Path((vehicle_id, id)): Path<(i32, i32)>`
- Filter by `Column::VehicleId.eq(vehicle_id)` on all queries
- Import and call `super::require_vehicle`
- For list endpoints with related data: use batch loading with `is_in()`, NOT per-record queries

**Critical update handler rules:**
- ALWAYS set `active.updated_at = Set(chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string())`
- Use `Option<Option<T>>` for nullable fields in UpdateDTO
- Add `#[allow(clippy::too_many_lines)]` if update has many fields

### 4. Register Handler — `src/api/mod.rs`

Add `pub mod {name};` in alphabetical order.

### 5. Route Registration — `src/main.rs`

**Top-level**: Add `.nest("/api/{kebab-plural}", api::{name}::router())`

**Vehicle sub-resource**: Add flat `.route()` calls:
```rust
.route(
    "/api/vehicles/{vehicle_id}/{kebab-plural}",
    get(api::{name}::list).post(api::{name}::create),
)
.route(
    "/api/vehicles/{vehicle_id}/{kebab-plural}/{id}",
    get(api::{name}::get_one).put(api::{name}::update),
)
```

### 6. TypeScript Types — `frontend/src/lib/types.ts`

```typescript
export interface {PascalName} {
  id: number
  // fields matching Rust Model
  // Option<T> → T | null
  // i32 → number
  // String → string
  created_at: string
  updated_at: string
}

export interface Create{PascalName} {
  // Required fields (bare types)
  // Optional fields: field?: type | null
  // Omit: id, created_at, updated_at
}
```

### 7. API Client — `frontend/src/lib/api.ts`

**Top-level**:
```typescript
export const {camelPlural} = {
  list: () => request<{PascalName}[]>('/{kebab-plural}'),
  get: (id: number) => request<{PascalName}>(`/{kebab-plural}/${id}`),
  create: (data: Create{PascalName}) =>
    request<{PascalName}>('/{kebab-plural}', { method: 'POST', body: JSON.stringify(data) }),
  update: (id: number, data: Partial<{PascalName}>) =>
    request<{PascalName}>(`/{kebab-plural}/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
}
```

**Vehicle sub-resource**: Same but with `vehicleId: number` param and `/vehicles/${vehicleId}/...` paths.

### 8. Import Types in `api.ts`

Add the new type to the import at the top of `api.ts` from `./types`.

---

## After Scaffold

1. Run `cargo clippy -- -D clippy::pedantic` to verify zero warnings
2. Run `cargo build` to check compilation
3. If a migration is needed, use the `/new-migration` skill
4. Consider if a Playwright test is needed — use `/new-e2e-test` skill
