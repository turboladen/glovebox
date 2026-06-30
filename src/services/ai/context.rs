use sea_orm::*;
use std::fmt::Write;

use crate::entities::{observation, part, part_slot, service_record, vehicle};
use crate::services::reminders;

/// Shared preamble for all AI system prompts so the bot knows it's part of Glovebox.
pub const GLOVEBOX_PREAMBLE: &str = "\
You are the AI assistant built into Glovebox, a car maintenance tracking application. \
The user is currently using Glovebox right now — never tell them to 'log this in your app' \
or 'check your records elsewhere.' They are already in the app.\n\
\n\
Glovebox capabilities and where to find them on the vehicle detail page:\n\
- Log a service record: click the \"Log Service\" button at the top, or go to the History tab\n\
- Update mileage: click the \"Update Mileage\" button at the top\n\
- View/manage maintenance schedule: Schedule tab (shows reminders for overdue/upcoming items)\n\
- Track parts and part slots: Parts tab\n\
- Record observations (noises, warning lights, issues): Observations tab\n\
- Upload and manage documents/invoices: Documents tab\n\
- Review cost of ownership: Costs tab\n\
- Check NHTSA recalls and research: Research tab\n\
- Chat with AI (you are here): Chat tab\n\
\n\
When suggesting actions, direct the user to the specific tab or button within Glovebox.\
";

/// Instructions appended to the chat system prompt when the conversation is
/// scoped to a vehicle. Tells the AI how to propose structured data that the
/// frontend can render as editable cards for one-click record creation.
pub const DATA_ENTRY_INSTRUCTIONS: &str = r#"

## Structured Data Entry

When the user mentions service work, parts purchases, or vehicle issues, you SHOULD extract structured data and return a `glovebox_actions` JSON block at the END of your response (after your natural-language explanation).

IMPORTANT: Only include the JSON block when there is concrete, actionable data to extract. Do NOT include it for general questions or advice.

The JSON block must be fenced in a ```glovebox_actions code block:

```glovebox_actions
{"glovebox_actions": {
  "service_records": [...],
  "parts": [...],
  "observations": [...]
}}
```

### Field schemas

**service_records** — each object:
- `service_date` (string, REQUIRED, "YYYY-MM-DD")
- `mileage` (integer or null, odometer reading)
- `description` (string or null, brief summary)
- `parts_cost_cents` (integer or null, total parts cost in cents)
- `labor_cost_cents` (integer or null, total labor cost in cents)
- `total_cost_cents` (integer or null, grand total in cents — multiply dollars by 100)
- `shop_name` (string or null)
- `notes` (string or null)
- `schedule_item_ids` (array of integers or null — IDs of maintenance schedule items this service fulfills; use the `[id=N]` values from Maintenance Status above when the service clearly covers those scheduled items, e.g. an oil change covers the "Oil Change" schedule item)
- `line_items` (array or null — itemized breakdown of work/parts/fees):
  - `description` (string, REQUIRED)
  - `category` (string or null: "part", "labor", "fee", "tax", "other")
  - `quantity` (number or null)
  - `unit_cost_cents` (integer or null)
  - `cost_cents` (integer or null)

**parts** — each object:
- `name` (string, REQUIRED, e.g. "Mobil 1 0W-40 Full Synthetic")
- `manufacturer` (string or null)
- `part_number` (string or null)
- `status` (string, default "installed")
- `installed_date` (string or null, "YYYY-MM-DD")
- `installed_odometer` (integer or null)
- `cost_cents` (integer or null, unit cost in cents)
- `seller` (string or null, where purchased)
- `notes` (string or null)

**observations** — each object:
- `category` (string, REQUIRED, one of: "noise", "vibration", "warning_light", "smell", "visual", "performance", "other")
- `title` (string, REQUIRED, short summary)
- `description` (string or null, details)
- `odometer` (integer or null)
- `obd_codes` (string or null, comma-separated codes like "P0301, P0302")
- `notes` (string or null)

### Example

User: "Got an oil change today at 45,000 miles, $75 at Joe's Auto. Used Mobil 1 0W-40."

Your response should explain what you extracted, then end with:

```glovebox_actions
{"glovebox_actions": {
  "service_records": [{"service_date": "2026-03-11", "mileage": 45000, "description": "Oil change", "total_cost_cents": 7500, "shop_name": "Joe's Auto", "schedule_item_ids": [3], "line_items": [{"description": "Oil filter", "category": "part", "quantity": 1, "cost_cents": 1200}, {"description": "Synthetic oil 5qt", "category": "part", "quantity": 1, "cost_cents": 3500}, {"description": "Labor", "category": "labor", "cost_cents": 2800}]}],
  "parts": [{"name": "Mobil 1 0W-40 Full Synthetic", "manufacturer": "Mobil", "status": "installed", "installed_date": "2026-03-11", "installed_odometer": 45000}],
  "observations": []
}}
```

Always use today's date if the user says "today" or doesn't specify a date. Convert all dollar amounts to cents (multiply by 100).

If a follow-up message confirms that suggested items were already created (e.g., "Created from suggestions:" followed by a bulleted list), do NOT re-propose those same items. Only suggest new actions based on new information.
"#;

/// Build a structured text context for a vehicle, suitable for AI system prompts.
/// Gathers vehicle details, recent services, installed parts, active observations,
/// and maintenance schedule status.
pub async fn build_vehicle_context(
    db: &DatabaseConnection,
    vehicle_id: i32,
) -> Result<String, DbErr> {
    let v = vehicle::Entity::find_by_id(vehicle_id)
        .one(db)
        .await?
        .ok_or_else(|| DbErr::RecordNotFound(format!("Vehicle {vehicle_id}")))?;

    let mut ctx = String::with_capacity(4096);

    // Vehicle info
    write_vehicle_info(&mut ctx, &v);

    // Mileage estimate
    let (est_mileage, mileage_as_of, avg_daily, is_estimate) =
        reminders::estimate_mileage(db, vehicle_id, &v).await?;
    let mileage_label = if is_estimate {
        "Estimated Mileage"
    } else {
        "Current Mileage"
    };
    writeln!(
        ctx,
        "{mileage_label}: {est_mileage} (as of {mileage_as_of})"
    )
    .unwrap();
    writeln!(ctx, "Average Daily Miles: {avg_daily:.1}").unwrap();
    writeln!(ctx).unwrap();

    // Recent service history
    write_services(&mut ctx, db, vehicle_id).await?;

    // Installed parts
    write_parts(&mut ctx, db, vehicle_id).await?;

    // Active observations
    write_observations(&mut ctx, db, vehicle_id).await?;

    // Maintenance schedule reminders
    write_reminders(&mut ctx, db, vehicle_id).await?;

    Ok(ctx)
}

fn write_vehicle_info(ctx: &mut String, v: &vehicle::Model) {
    writeln!(ctx, "Vehicle: {}", v.name).unwrap();

    let mut specs = Vec::new();
    if let Some(y) = v.year {
        specs.push(format!("Year: {y}"));
    }
    if let Some(ref m) = v.make {
        specs.push(format!("Make: {m}"));
    }
    if let Some(ref m) = v.model {
        specs.push(format!("Model: {m}"));
    }
    if let Some(ref t) = v.trim_level {
        specs.push(format!("Trim: {t}"));
    }
    if let Some(ref e) = v.engine {
        specs.push(format!("Engine: {e}"));
    }
    if let Some(ref t) = v.transmission {
        specs.push(format!("Transmission: {t}"));
    }
    if let Some(ref d) = v.drivetrain {
        specs.push(format!("Drivetrain: {d}"));
    }
    if let Some(ref vin) = v.vin {
        specs.push(format!("VIN: {vin}"));
    }
    if !specs.is_empty() {
        writeln!(ctx, "{}", specs.join(", ")).unwrap();
    }

    if let Some(ref pd) = v.purchase_date {
        write!(ctx, "Purchased: {pd}").unwrap();
        if let Some(pm) = v.purchase_mileage {
            write!(ctx, " at {pm} miles").unwrap();
        }
        writeln!(ctx).unwrap();
    }
}

async fn write_services(
    ctx: &mut String,
    db: &DatabaseConnection,
    vehicle_id: i32,
) -> Result<(), DbErr> {
    let services = service_record::Entity::find()
        .filter(service_record::Column::VehicleId.eq(vehicle_id))
        .order_by_desc(service_record::Column::ServiceDate)
        .limit(20)
        .all(db)
        .await?;

    if services.is_empty() {
        writeln!(ctx, "Service History: No service records.").unwrap();
        writeln!(ctx).unwrap();
        return Ok(());
    }

    writeln!(ctx, "Recent Service History (last {}):", services.len()).unwrap();
    for svc in &services {
        write!(ctx, "- {}", svc.service_date).unwrap();
        if let Some(m) = svc.mileage {
            write!(ctx, " @ {m}mi").unwrap();
        }
        write!(
            ctx,
            ": {}",
            svc.description.as_deref().unwrap_or("(no description)")
        )
        .unwrap();
        if let Some(cost) = svc.total_cost_cents {
            write!(ctx, " (${:.2})", f64::from(cost) / 100.0).unwrap();
        }
        if let Some(ref shop) = svc.shop_name {
            write!(ctx, " at {shop}").unwrap();
        }
        writeln!(ctx).unwrap();
    }
    writeln!(ctx).unwrap();

    Ok(())
}

async fn write_parts(
    ctx: &mut String,
    db: &DatabaseConnection,
    vehicle_id: i32,
) -> Result<(), DbErr> {
    let slots = part_slot::Entity::find()
        .filter(part_slot::Column::VehicleId.eq(vehicle_id))
        .all(db)
        .await?;

    let installed_parts = part::Entity::find()
        .filter(part::Column::VehicleId.eq(vehicle_id))
        .filter(part::Column::Status.eq("installed"))
        .all(db)
        .await?;

    if installed_parts.is_empty() {
        writeln!(ctx, "Installed Parts: None.").unwrap();
        writeln!(ctx).unwrap();
        return Ok(());
    }

    writeln!(ctx, "Installed Parts:").unwrap();
    for p in &installed_parts {
        let slot_name = p
            .slot_id
            .and_then(|sid| slots.iter().find(|s| s.id == sid))
            .map_or("unslotted", |s| s.name.as_str());

        write!(ctx, "- [{slot_name}] {}", p.name).unwrap();
        if let Some(ref mfr) = p.manufacturer {
            write!(ctx, " ({mfr})").unwrap();
        }
        if let Some(ref d) = p.installed_date {
            write!(ctx, ", installed {d}").unwrap();
        }
        if let Some(m) = p.installed_odometer {
            write!(ctx, " @ {m}mi").unwrap();
        }
        writeln!(ctx).unwrap();
    }
    writeln!(ctx).unwrap();

    Ok(())
}

async fn write_observations(
    ctx: &mut String,
    db: &DatabaseConnection,
    vehicle_id: i32,
) -> Result<(), DbErr> {
    let obs = observation::Entity::find()
        .filter(observation::Column::VehicleId.eq(vehicle_id))
        .filter(observation::Column::Resolved.eq(false))
        .order_by_desc(observation::Column::ObservedAt)
        .all(db)
        .await?;

    if obs.is_empty() {
        writeln!(ctx, "Active Observations: None.").unwrap();
        writeln!(ctx).unwrap();
        return Ok(());
    }

    writeln!(ctx, "Active Observations (unresolved):").unwrap();
    for o in &obs {
        write!(ctx, "- [{}] {}", o.category, o.title).unwrap();
        if let Some(ref desc) = o.description {
            write!(ctx, " — {desc}").unwrap();
        }
        write!(ctx, " (observed {})", o.observed_at).unwrap();
        if let Some(ref codes) = o.obd_codes {
            write!(ctx, " OBD: {codes}").unwrap();
        }
        writeln!(ctx).unwrap();
    }
    writeln!(ctx).unwrap();

    Ok(())
}

async fn write_reminders(
    ctx: &mut String,
    db: &DatabaseConnection,
    vehicle_id: i32,
) -> Result<(), DbErr> {
    // Reuse the existing reminder engine
    let reminder_resp = reminders::calculate_reminders(db, vehicle_id).await?;

    if reminder_resp.reminders.is_empty() {
        writeln!(ctx, "Maintenance Schedule: No schedule items.").unwrap();
        return Ok(());
    }

    writeln!(ctx, "Maintenance Status:").unwrap();
    for r in &reminder_resp.reminders {
        write!(
            ctx,
            "- [id={}] {}: {}",
            r.schedule_item.id,
            r.schedule_item.name,
            r.status.to_uppercase()
        )
        .unwrap();

        if let Some(miles) = r.miles_remaining {
            write!(ctx, " ({miles} miles remaining)").unwrap();
        }
        if let Some(days) = r.days_remaining {
            write!(ctx, " ({days} days remaining)").unwrap();
        }
        if let Some(ref ls) = r.last_service {
            write!(ctx, " [last: {}", ls.date).unwrap();
            if let Some(m) = ls.odometer {
                write!(ctx, " @ {m}mi").unwrap();
            }
            write!(ctx, "]").unwrap();
        }
        writeln!(ctx).unwrap();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_vehicle_info_full() {
        let v = make_test_vehicle();
        let mut ctx = String::new();
        write_vehicle_info(&mut ctx, &v);

        assert!(ctx.contains("Vehicle: Test GTI"));
        assert!(ctx.contains("Year: 2017"));
        assert!(ctx.contains("Make: Volkswagen"));
        assert!(ctx.contains("Engine: 2.0L TSI"));
        assert!(ctx.contains("Purchased: 2020-01-15 at 45000 miles"));
    }

    #[test]
    fn write_vehicle_info_minimal() {
        let v = vehicle::Model {
            id: 1,
            model_template_id: None,
            name: "Bare Car".into(),
            year: None,
            make: None,
            model: None,
            trim_level: None,
            body_style: None,
            engine: None,
            transmission: None,
            drivetrain: None,
            vin: None,
            license_plate: None,
            color: None,
            purchase_date: None,
            purchase_price_cents: None,
            purchase_price_currency: None,
            purchase_mileage: None,
            sold_date: None,
            sold_price_cents: None,
            sold_price_currency: None,
            sold_mileage: None,
            photo_path: None,
            notes: None,
            created_at: "2026-01-01".into(),
            updated_at: "2026-01-01".into(),
            archived_at: None,
        };
        let mut ctx = String::new();
        write_vehicle_info(&mut ctx, &v);

        assert!(ctx.contains("Vehicle: Bare Car"));
        assert!(!ctx.contains("Year:"));
        assert!(!ctx.contains("Purchased:"));
    }

    #[test]
    fn preamble_identifies_glovebox() {
        assert!(GLOVEBOX_PREAMBLE.contains("Glovebox"));
        assert!(GLOVEBOX_PREAMBLE.contains("never tell them"));
        // Verify it mentions the key tabs/actions
        assert!(GLOVEBOX_PREAMBLE.contains("Log Service"));
        assert!(GLOVEBOX_PREAMBLE.contains("Update Mileage"));
        assert!(GLOVEBOX_PREAMBLE.contains("Schedule tab"));
        assert!(GLOVEBOX_PREAMBLE.contains("Parts tab"));
        assert!(GLOVEBOX_PREAMBLE.contains("Observations tab"));
        assert!(GLOVEBOX_PREAMBLE.contains("Documents tab"));
        assert!(GLOVEBOX_PREAMBLE.contains("Costs tab"));
        assert!(GLOVEBOX_PREAMBLE.contains("Research tab"));
    }

    fn make_test_vehicle() -> vehicle::Model {
        vehicle::Model {
            id: 1,
            model_template_id: Some(1),
            name: "Test GTI".into(),
            year: Some(2017),
            make: Some("Volkswagen".into()),
            model: Some("Golf GTI".into()),
            trim_level: Some("SE".into()),
            body_style: None,
            engine: Some("2.0L TSI".into()),
            transmission: Some("6MT".into()),
            drivetrain: Some("FWD".into()),
            vin: Some("WVWAB7AU1HK123456".into()),
            license_plate: None,
            color: None,
            purchase_date: Some("2020-01-15".into()),
            purchase_price_cents: None,
            purchase_price_currency: None,
            purchase_mileage: Some(45000),
            sold_date: None,
            sold_price_cents: None,
            sold_price_currency: None,
            sold_mileage: None,
            photo_path: None,
            notes: None,
            created_at: "2026-01-01".into(),
            updated_at: "2026-01-01".into(),
            archived_at: None,
        }
    }
}
