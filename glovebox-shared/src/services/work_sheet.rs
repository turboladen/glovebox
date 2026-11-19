//! The work sheet the owner hands the mechanic for an appointment: each
//! planned job, the parts the owner brings and the parts the shop supplies,
//! and the needs not settled enough to print.

use chrono::NaiveDate;
use sea_orm::*;
use serde::Serialize;

use crate::{
    entities::{part::Supplier, part_content, work_item::JobStatus},
    error::{DomainError, DomainResult},
    services::{
        checklist::{self, NeedView},
        need::Stage,
        part::{self as part_svc, PartView},
        vehicle,
    },
};

/// An appointment's work sheet.
#[derive(Debug, Serialize)]
pub struct WorkSheet {
    pub appointment_id: i32,
    /// The vehicle's name.
    pub vehicle: String,
    pub shop: Option<String>,
    pub date: Option<NaiveDate>,
    /// The appointment's planned jobs, in checklist order.
    pub jobs: Vec<SheetJob>,
    /// The needs of planned jobs that are undecided or not yet ordered. They
    /// are on no job's lists.
    pub unsettled: Vec<Unsettled>,
}

/// One planned job with the parts each side supplies.
#[derive(Debug, Serialize)]
pub struct SheetJob {
    pub job_id: i32,
    pub title: String,
    pub owner_supplies: Vec<SheetLine>,
    pub shop_supplies: Vec<SheetLine>,
}

/// One part on a job's list.
#[derive(Debug, Serialize)]
pub struct SheetLine {
    pub need_id: i32,
    /// The bought part on an owner line. A shop line never carries one.
    pub part_id: Option<i32>,
    /// The bought part's name on an owner line, and the need's on a shop line.
    pub name: String,
    pub quantity: i32,
    /// The manufacturer and part number together, such as `Mann HU 7020 z`,
    /// as [`part_svc::label`] writes them. A shop line never carries one.
    pub label: Option<String>,
    /// The items inside an owner's kit, in print order. A shop line never
    /// carries any.
    pub contents: Vec<SheetContent>,
}

/// One item inside a kit the owner brings.
#[derive(Debug, Serialize)]
pub struct SheetContent {
    /// The item's id, which `correct_part` takes to remove it.
    pub id: i32,
    pub name: String,
    pub quantity: i32,
    /// The item's manufacturer and number, as [`part_svc::label`] writes them.
    pub label: Option<String>,
}

impl From<&part_content::Model> for SheetContent {
    fn from(item: &part_content::Model) -> Self {
        SheetContent {
            id: item.id,
            label: part_svc::label(item.manufacturer.as_deref(), item.part_number.as_deref()),
            name: item.name.clone(),
            quantity: item.quantity,
        }
    }
}

/// A need that is not settled enough to print.
#[derive(Debug, Serialize)]
pub struct Unsettled {
    pub need_id: i32,
    pub job_id: i32,
    pub job_title: String,
    pub name: String,
    /// The need's stage, which is `undecided` or `to_order`.
    pub stage: Stage,
    /// What is asked of the owner, as the checklist says it.
    pub detail: String,
}

/// The work sheet of one appointment. Installed needs, and every need of a
/// done or dropped job, are left off.
///
/// # Errors
/// `NotFound` for an appointment that is not the vehicle's.
pub async fn for_appointment(
    db: &impl ConnectionTrait,
    vehicle_id: i32,
    appointment_id: i32,
) -> DomainResult<WorkSheet> {
    // The sheet keeps only the appointment's row, so the day does not matter.
    let list = checklist::for_appointment(db, vehicle_id, appointment_id, crate::services::today())
        .await?;
    let car = vehicle::require(db, vehicle_id).await?;
    let appointment = list
        .appointment
        .ok_or_else(|| DomainError::Internal("an appointment checklist has no appointment".into()))?
        .appointment;
    let mut jobs = Vec::new();
    let mut unsettled = Vec::new();
    for view in list.jobs {
        if view.job.status != JobStatus::Planned {
            continue;
        }
        let mut job = SheetJob {
            job_id: view.job.id,
            title: view.job.title.clone(),
            owner_supplies: Vec::new(),
            shop_supplies: Vec::new(),
        };
        for need in &view.needs {
            match placement(need) {
                Placement::Off => {}
                // The name and label come from the part's own record, so the
                // sheet never prints a researched option's number beside a
                // part that may not be that option.
                Placement::Owner(PartView { part, contents }) => {
                    job.owner_supplies.push(SheetLine {
                        need_id: need.need.id,
                        part_id: Some(part.id),
                        name: part.name.clone(),
                        quantity: need.need.quantity,
                        label: part_svc::label(
                            part.manufacturer.as_deref(),
                            part.part_number.as_deref(),
                        ),
                        contents: contents.iter().map(SheetContent::from).collect(),
                    });
                }
                Placement::Shop => job.shop_supplies.push(line(need)),
                Placement::Unsettled => unsettled.push(Unsettled {
                    need_id: need.need.id,
                    job_id: view.job.id,
                    job_title: view.job.title.clone(),
                    name: need.need.name.clone(),
                    stage: need.stage,
                    detail: checklist::need_detail(need, JobStatus::Planned),
                }),
            }
        }
        jobs.push(job);
    }
    Ok(WorkSheet {
        appointment_id,
        vehicle: car.name,
        shop: appointment.shop_name,
        date: appointment.planned_date,
        jobs,
        unsettled,
    })
}

enum Placement<'a> {
    Off,
    Owner(&'a PartView),
    Shop,
    Unsettled,
}

// A linked part decides the side, so a shop part recorded against a need
// prints under the shop's list whatever the need says.
fn placement(view: &NeedView) -> Placement<'_> {
    match (view.stage, &view.part) {
        (Stage::Installed, _) => Placement::Off,
        (_, Some(part)) => match part.part.supplied_by {
            Supplier::Owner => Placement::Owner(part),
            Supplier::Shop => Placement::Shop,
        },
        (Stage::ShopSupplies, None) => Placement::Shop,
        _ => Placement::Unsettled,
    }
}

fn line(view: &NeedView) -> SheetLine {
    SheetLine {
        need_id: view.need.id,
        part_id: None,
        name: view.need.name.clone(),
        quantity: view.need.quantity,
        label: None,
        contents: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::DatabaseConnection;

    use super::*;
    use crate::{
        entities::{part::Supplier, work_item::JobStatus},
        inputs::{
            appointment::NewAppointment,
            need::{NewNeed, NewOption, OrderDetails, UpdateNeed},
            part::{NewPart, NewPartContent},
        },
        services::{appointment, checklist, need, need::Stage, part as part_svc},
        test_support::{VehicleFixture, add_job, assert_empty, date, set_job_status, test_db},
    };

    async fn appointment_on(db: &DatabaseConnection, vehicle_id: i32) -> i32 {
        appointment::create(
            db,
            vehicle_id,
            NewAppointment {
                purpose: "60k service".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    async fn new_need(db: &DatabaseConnection, v: i32, job: i32, name: &str, quantity: i32) -> i32 {
        need::add(
            db,
            v,
            job,
            NewNeed {
                name: name.into(),
                quantity: Some(quantity),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    async fn set_supplier(db: &DatabaseConnection, v: i32, need_id: i32, who: Supplier) {
        need::update(
            db,
            v,
            need_id,
            UpdateNeed {
                supplied_by: Some(Some(who)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    // A need the owner supplies, with an option chosen whose manufacturer and
    // number are as given.
    async fn chosen_need(
        db: &DatabaseConnection,
        v: i32,
        job: i32,
        name: &str,
        manufacturer: Option<&str>,
        number: Option<&str>,
    ) -> i32 {
        let id = new_need(db, v, job, name, 1).await;
        let option = need::add_option(
            db,
            v,
            id,
            NewOption {
                description: format!("{name} option"),
                manufacturer: manufacturer.map(Into::into),
                part_number: number.map(Into::into),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        set_supplier(db, v, id, Supplier::Owner).await;
        need::decide(db, v, id, Some(option), None).await.unwrap();
        id
    }

    // Links a recorded part to `need_id`, received on `received`.
    async fn link_part(
        db: &DatabaseConnection,
        v: i32,
        need_id: i32,
        part: NewPart,
        received: Option<&str>,
    ) {
        let part = part_svc::create(db, v, part).await.unwrap();
        need::link_part(db, v, need_id, part.id, received.map(date))
            .await
            .unwrap();
    }

    fn only_job(sheet: &WorkSheet) -> &SheetJob {
        assert_eq!(sheet.jobs.len(), 1, "{:?}", sheet.jobs);
        &sheet.jobs[0]
    }

    #[tokio::test]
    async fn received_owner_part_prints_with_its_own_number() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_on(&db, v).await;
        let job = add_job(&db, v, "Front brakes", Some(a)).await;
        let n = chosen_need(&db, v, job, "Brake pads", Some("Brembo"), Some("1")).await;
        link_part(
            &db,
            v,
            n,
            NewPart {
                name: "Brake pads".into(),
                manufacturer: Some("ATE".into()),
                part_number: Some("13.0460".into()),
                supplied_by: Some(Supplier::Owner),
                ..Default::default()
            },
            Some("2026-11-10"),
        )
        .await;

        let sheet = for_appointment(&db, v, a).await.unwrap();

        let job = only_job(&sheet);
        assert_eq!(job.owner_supplies.len(), 1);
        assert_eq!(job.owner_supplies[0].label.as_deref(), Some("ATE 13.0460"));
        assert_empty(&job.shop_supplies);
    }

    fn kit_item(name: &str, number: Option<&str>, quantity: Option<i32>) -> NewPartContent {
        NewPartContent {
            name: name.into(),
            manufacturer: number.map(|_| "Mann".into()),
            part_number: number.map(Into::into),
            quantity,
        }
    }

    #[tokio::test]
    async fn owner_line_prints_the_bought_parts_name() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_on(&db, v).await;
        let job = add_job(&db, v, "Plugs", Some(a)).await;
        let n = new_need(&db, v, job, "Spark plugs", 4).await;
        link_part(
            &db,
            v,
            n,
            NewPart {
                name: "Beru spark plugs".into(),
                supplied_by: Some(Supplier::Owner),
                ..Default::default()
            },
            Some("2026-11-10"),
        )
        .await;

        let sheet = for_appointment(&db, v, a).await.unwrap();

        let line = &only_job(&sheet).owner_supplies[0];
        assert_eq!(line.name, "Beru spark plugs");
        assert_eq!(line.quantity, 4);
    }

    #[tokio::test]
    async fn a_kit_lists_its_contents_in_order_with_their_numbers() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_on(&db, v).await;
        let job = add_job(&db, v, "Oil change", Some(a)).await;
        let n = new_need(&db, v, job, "Oil change kit", 1).await;
        link_part(
            &db,
            v,
            n,
            NewPart {
                name: "Oil change kit".into(),
                part_number: Some("KIT-1".into()),
                supplied_by: Some(Supplier::Owner),
                contents: vec![
                    kit_item("Oil filter", Some("HU 7020 z"), None),
                    kit_item("Engine oil, 1 L", None, Some(6)),
                    kit_item("Drain plug", Some("N 908 132 02"), None),
                ],
                ..Default::default()
            },
            Some("2026-11-10"),
        )
        .await;

        let sheet = for_appointment(&db, v, a).await.unwrap();

        let line = &only_job(&sheet).owner_supplies[0];
        assert_eq!(line.label.as_deref(), Some("KIT-1"));
        let printed: Vec<(&str, i32, Option<&str>)> = line
            .contents
            .iter()
            .map(|c| (c.name.as_str(), c.quantity, c.label.as_deref()))
            .collect();
        assert_eq!(
            printed,
            vec![
                ("Oil filter", 1, Some("Mann HU 7020 z")),
                ("Engine oil, 1 L", 6, None),
                ("Drain plug", 1, Some("Mann N 908 132 02")),
            ]
        );
    }

    #[tokio::test]
    async fn a_shop_line_lists_no_contents() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_on(&db, v).await;
        let job = add_job(&db, v, "Haldex service", Some(a)).await;
        let n = new_need(&db, v, job, "Haldex kit", 1).await;
        link_part(
            &db,
            v,
            n,
            NewPart {
                name: "Haldex kit".into(),
                supplied_by: Some(Supplier::Shop),
                contents: vec![kit_item("Haldex filter", Some("1"), None)],
                ..Default::default()
            },
            None,
        )
        .await;

        let sheet = for_appointment(&db, v, a).await.unwrap();

        assert_empty(&only_job(&sheet).shop_supplies[0].contents);
    }

    #[tokio::test]
    async fn ordered_part_prints_as_the_owners() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_on(&db, v).await;
        let job = add_job(&db, v, "Front brakes", Some(a)).await;
        let n = chosen_need(&db, v, job, "Rotors", Some("Zimmermann"), Some("100.3380")).await;
        need::mark_ordered(
            &db,
            v,
            n,
            OrderDetails {
                purchase_date: Some(date("2026-11-10")),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let sheet = for_appointment(&db, v, a).await.unwrap();

        let job = only_job(&sheet);
        assert_eq!(job.owner_supplies[0].name, "Rotors option");
        assert_eq!(
            job.owner_supplies[0].label.as_deref(),
            Some("Zimmermann 100.3380")
        );
        assert_empty(&sheet.unsettled);
    }

    // Links a part with `manufacturer` and no number to an oil filter whose
    // chosen option is Mann HU 7020 z, and returns the owner line's label.
    async fn label_for_unnumbered_part(manufacturer: Option<&str>) -> Option<String> {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_on(&db, v).await;
        let job = add_job(&db, v, "Oil change", Some(a)).await;
        let n = chosen_need(&db, v, job, "Oil filter", Some("Mann"), Some("HU 7020 z")).await;
        link_part(
            &db,
            v,
            n,
            NewPart {
                name: "Oil filter".into(),
                manufacturer: manufacturer.map(Into::into),
                supplied_by: Some(Supplier::Owner),
                ..Default::default()
            },
            None,
        )
        .await;
        let sheet = for_appointment(&db, v, a).await.unwrap();
        only_job(&sheet).owner_supplies[0].label.clone()
    }

    #[tokio::test]
    async fn an_unnumbered_part_never_borrows_the_options_number() {
        for brand in [None, Some(" mann "), Some("Bosch")] {
            assert_eq!(label_for_unnumbered_part(brand).await, None, "{brand:?}");
        }
    }

    #[tokio::test]
    async fn manufacturer_without_number_prints_no_label() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_on(&db, v).await;
        let job = add_job(&db, v, "Oil change", Some(a)).await;
        let n = chosen_need(&db, v, job, "Oil filter", Some("Mann"), None).await;
        need::mark_ordered(&db, v, n, OrderDetails::default())
            .await
            .unwrap();

        let sheet = for_appointment(&db, v, a).await.unwrap();

        let line = &only_job(&sheet).owner_supplies[0];
        assert_eq!(line.name, "Oil filter option");
        assert_eq!(line.label, None);
    }

    #[tokio::test]
    async fn shop_part_linked_to_a_need_goes_under_please_supply() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_on(&db, v).await;
        let job = add_job(&db, v, "Front brakes", Some(a)).await;
        let n = new_need(&db, v, job, "Brake fluid", 1).await;
        link_part(
            &db,
            v,
            n,
            NewPart {
                name: "DOT 4".into(),
                part_number: Some("X1".into()),
                supplied_by: Some(Supplier::Shop),
                ..Default::default()
            },
            Some("2026-11-10"),
        )
        .await;

        let sheet = for_appointment(&db, v, a).await.unwrap();

        let job = only_job(&sheet);
        assert_empty(&job.owner_supplies);
        assert_eq!(job.shop_supplies[0].name, "Brake fluid");
        assert_eq!(job.shop_supplies[0].label, None);
    }

    #[tokio::test]
    async fn shop_supplied_need_has_name_only() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_on(&db, v).await;
        let job = add_job(&db, v, "Oil change", Some(a)).await;
        let n = new_need(&db, v, job, "Oil", 6).await;
        set_supplier(&db, v, n, Supplier::Shop).await;

        let sheet = for_appointment(&db, v, a).await.unwrap();

        let job = only_job(&sheet);
        assert_eq!(job.shop_supplies.len(), 1);
        assert_eq!(job.shop_supplies[0].name, "Oil");
        assert_eq!(job.shop_supplies[0].label, None);
        assert_empty(&sheet.unsettled);
    }

    #[tokio::test]
    async fn undecided_and_to_order_needs_are_unsettled() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_on(&db, v).await;
        let job = add_job(&db, v, "Wipers", Some(a)).await;
        let undecided = new_need(&db, v, job, "Wiper blades", 1).await;
        let to_order = chosen_need(&db, v, job, "Cabin filter", None, Some("CU 1")).await;

        let sheet = for_appointment(&db, v, a).await.unwrap();
        let list = checklist::for_appointment(&db, v, a, crate::services::today())
            .await
            .unwrap();

        let job = only_job(&sheet);
        assert_empty(&job.owner_supplies);
        assert_empty(&job.shop_supplies);
        let ids: Vec<i32> = sheet.unsettled.iter().map(|u| u.need_id).collect();
        assert_eq!(ids, vec![undecided, to_order]);
        assert_eq!(sheet.unsettled[0].stage, Stage::Undecided);
        assert_eq!(sheet.unsettled[1].stage, Stage::ToOrder);
        assert_eq!(sheet.unsettled[0].job_title, "Wipers");
        for (u, view) in sheet.unsettled.iter().zip(&list.jobs[0].needs) {
            assert_eq!(u.detail, checklist::need_detail(view, JobStatus::Planned));
        }
    }

    #[tokio::test]
    async fn installed_needs_are_left_off() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_on(&db, v).await;
        let job = add_job(&db, v, "Cabin filter", Some(a)).await;
        let n = new_need(&db, v, job, "Cabin filter", 1).await;
        link_part(
            &db,
            v,
            n,
            NewPart {
                name: "Cabin filter".into(),
                fitting: crate::inputs::part::NewFitting::Installed(crate::inputs::part::Install {
                    date: Some(date("2026-11-01")),
                    ..Default::default()
                }),
                supplied_by: Some(Supplier::Owner),
                ..Default::default()
            },
            None,
        )
        .await;

        let sheet = for_appointment(&db, v, a).await.unwrap();

        let job = only_job(&sheet);
        assert_eq!(job.title, "Cabin filter");
        assert_empty(&job.owner_supplies);
        assert_empty(&job.shop_supplies);
        assert_empty(&sheet.unsettled);
    }

    #[tokio::test]
    async fn done_and_dropped_jobs_are_left_off() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_on(&db, v).await;
        let done = add_job(&db, v, "Done job", Some(a)).await;
        let dropped = add_job(&db, v, "Dropped job", Some(a)).await;
        let planned = add_job(&db, v, "Planned job", Some(a)).await;
        new_need(&db, v, done, "Done need", 1).await;
        new_need(&db, v, dropped, "Dropped need", 1).await;
        set_job_status(&db, v, done, JobStatus::Done).await;
        set_job_status(&db, v, dropped, JobStatus::Dropped).await;

        let sheet = for_appointment(&db, v, a).await.unwrap();

        let ids: Vec<i32> = sheet.jobs.iter().map(|j| j.job_id).collect();
        assert_eq!(ids, vec![planned]);
        assert_empty(&sheet.unsettled);
    }

    #[tokio::test]
    async fn quantity_is_carried() {
        let db = test_db().await;
        let v = VehicleFixture::new().insert_id(&db).await;
        let a = appointment_on(&db, v).await;
        let job = add_job(&db, v, "Front brakes", Some(a)).await;
        let n = new_need(&db, v, job, "Rotors", 2).await;
        set_supplier(&db, v, n, Supplier::Shop).await;

        let sheet = for_appointment(&db, v, a).await.unwrap();

        assert_eq!(only_job(&sheet).shop_supplies[0].quantity, 2);
    }

    #[tokio::test]
    async fn header_names_the_vehicle_shop_and_date() {
        let db = test_db().await;
        let v = VehicleFixture::new().name("Golf GTI").insert_id(&db).await;
        let a = appointment::create(
            &db,
            v,
            NewAppointment {
                purpose: "Brakes".into(),
                shop_name: Some("Smith Auto".into()),
                planned_date: Some(date("2026-11-20")),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;

        let sheet = for_appointment(&db, v, a).await.unwrap();

        assert_eq!(sheet.appointment_id, a);
        assert_eq!(sheet.vehicle, "Golf GTI");
        assert_eq!(sheet.shop.as_deref(), Some("Smith Auto"));
        assert_eq!(sheet.date, Some(date("2026-11-20")));
    }

    #[tokio::test]
    async fn another_vehicles_appointment_is_not_found() {
        let db = test_db().await;
        let mine = VehicleFixture::new().insert_id(&db).await;
        let other = VehicleFixture::new().name("Other").insert_id(&db).await;
        let a = appointment_on(&db, other).await;

        let wrong = for_appointment(&db, mine, a).await.unwrap_err().to_string();
        let missing = for_appointment(&db, mine, 9999)
            .await
            .unwrap_err()
            .to_string()
            .replace("9999", &a.to_string());

        assert_eq!(wrong, missing);
    }
}
