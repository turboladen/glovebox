use sea_orm::*;
use std::fmt::Write;

use crate::entities::{observation, part, part_slot, service_record, vehicle};
use crate::services::reminders;

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
    let (est_mileage, mileage_as_of, avg_daily) =
        reminders::estimate_mileage(db, vehicle_id, &v).await?;
    writeln!(ctx, "Estimated Mileage: {est_mileage} (as of {mileage_as_of})").unwrap();
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
        write!(ctx, ": {}", svc.description.as_deref().unwrap_or("(no description)")).unwrap();
        if let Some(cost) = svc.total_cost_cents {
            write!(ctx, " (${:.2})", cost as f64 / 100.0).unwrap();
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
            .map(|s| s.name.as_str())
            .unwrap_or("unslotted");

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
            "- {}: {}",
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
        };
        let mut ctx = String::new();
        write_vehicle_info(&mut ctx, &v);

        assert!(ctx.contains("Vehicle: Bare Car"));
        assert!(!ctx.contains("Year:"));
        assert!(!ctx.contains("Purchased:"));
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
        }
    }
}
