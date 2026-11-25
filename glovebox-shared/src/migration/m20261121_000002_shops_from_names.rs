//! Moves every free-text shop name on services and appointments into the shop
//! directory: each row then names a shop by `shop_id` and its `shop_name` is
//! empty, and a CHECK keeps it empty.
//!
//! A trailing `(…)` in a stored name that holds a digit is the shop's
//! address. Names that are
//! equal apart from ASCII case and surrounding spaces become one shop unless
//! their addresses differ, and then each address is a shop of its own. Names
//! that differ, such as one dealer under two names, stay two shops for the
//! owner to merge.

use std::collections::HashMap;

use sea_orm::{DatabaseBackend, Statement, Value};
use sea_orm_migration::prelude::*;

use super::schema::{Check, add_checks, scalar};

// The rules as this migration adds them. A rule changes only through a new
// migration, because `has_check` finds a constraint by its name and a fresh
// database must end up with what a migrated one has.
const CHECKS: &[Check] = &[
    Check {
        table: "service_records",
        name: "service_records_shop_name_is_empty",
        holds: "shop_name IS NULL",
    },
    Check {
        table: "appointments",
        name: "appointments_shop_name_is_empty",
        holds: "shop_name IS NULL",
    },
];

const TABLES: [&str; 2] = ["service_records", "appointments"];

#[derive(DeriveMigrationName)]
pub struct Migration;

/// A stored name split into the shop's name and, from a trailing `(…)`, its
/// address.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ShopText {
    name: String,
    address: Option<String>,
}

fn split_name_and_address(text: &str) -> Option<ShopText> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let split = text
        .strip_suffix(')')
        .and_then(|rest| {
            rest.rfind('(')
                .map(|open| (&rest[..open], &rest[open + 1..]))
        })
        .map(|(name, address)| (name.trim(), address.trim()))
        // A street address holds a number, so a remark such as "(oil only)"
        // stays part of the name.
        .filter(|(name, address)| !name.is_empty() && address.chars().any(|c| c.is_ascii_digit()));
    Some(match split {
        Some((name, address)) => ShopText {
            name: name.to_owned(),
            address: Some(address.to_owned()),
        },
        None => ShopText {
            name: text.to_owned(),
            address: None,
        },
    })
}

// The comparison `shop::text_key` makes, written out here so that this
// migration keeps doing what it did when it ran, whatever the service's rule
// becomes.
fn key(text: &str) -> String {
    text.trim().to_ascii_lowercase()
}

/// The shop each stored name becomes, in the order the names are given.
/// Names that share a key share a shop, which takes the first spelling and
/// the group's address, unless the group holds two addresses: then each
/// address is its own shop and the entries with none share one more.
fn plan_shops(texts: &[ShopText]) -> Vec<ShopText> {
    let mut addresses: HashMap<String, Vec<String>> = HashMap::new();
    let mut spelling: HashMap<String, String> = HashMap::new();
    for text in texts {
        let name_key = key(&text.name);
        spelling
            .entry(name_key.clone())
            .or_insert_with(|| text.name.clone());
        let known = addresses.entry(name_key).or_default();
        if let Some(address) = &text.address
            && !known.iter().any(|a| key(a) == key(address))
        {
            known.push(address.clone());
        }
    }
    texts
        .iter()
        .map(|text| {
            let name_key = key(&text.name);
            let known = &addresses[&name_key];
            let address = match (known.as_slice(), &text.address) {
                ([only], _) => Some(only.clone()),
                (_, Some(address)) => known.iter().find(|a| key(a) == key(address)).cloned(),
                _ => None,
            };
            ShopText {
                name: spelling[&name_key].clone(),
                address,
            }
        })
        .collect()
}

// Whether both tables hold a `shop_name` column.
async fn tables_hold_names(db: &impl ConnectionTrait) -> Result<bool, DbErr> {
    for table in TABLES {
        let held = scalar::<bool>(
            db,
            &format!(
                "SELECT count(*) > 0 FROM pragma_table_info('{table}') WHERE name = 'shop_name'"
            ),
        )
        .await?;
        if !held {
            return Ok(false);
        }
    }
    Ok(true)
}

struct Named {
    table: &'static str,
    id: i32,
    shop_id: Option<i32>,
    text: String,
}

async fn named_rows(db: &impl ConnectionTrait) -> Result<Vec<Named>, DbErr> {
    let mut out = Vec::new();
    for table in TABLES {
        for row in db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!(
                    "SELECT id, shop_id, shop_name FROM {table} WHERE shop_name IS NOT NULL ORDER \
                     BY id"
                ),
            ))
            .await?
        {
            out.push(Named {
                table,
                id: row.try_get_by_index(0)?,
                shop_id: row.try_get_by_index(1)?,
                text: row.try_get_by_index(2)?,
            });
        }
    }
    Ok(out)
}

// The shops by the key of their name and address. `stored` holds the shops
// the directory held before the upgrade, and `created` the ones it adds.
#[derive(Default)]
struct Directory {
    stored: Vec<(i32, String, Option<String>)>,
    created: HashMap<(String, Option<String>), i32>,
}

fn directory_key(shop: &ShopText) -> (String, Option<String>) {
    (key(&shop.name), shop.address.as_deref().map(key))
}

impl Directory {
    // A shop the owner added matches by the directory's own rule: the same
    // name, and the same address or either one missing. A shop this upgrade
    // adds matches its planned name and address exactly, as `plan_shops`
    // already joined what belongs together.
    fn find(&self, shop: &ShopText) -> Option<i32> {
        let (name, address) = directory_key(shop);
        let same_name: Vec<&(i32, String, Option<String>)> =
            self.stored.iter().filter(|(_, n, _)| *n == name).collect();
        let exact = same_name.iter().find(|(_, _, a)| *a == address);
        // The loose match holds only when it names one shop: with two stored
        // shops of the name, a missing address cannot say which one is meant.
        let loose = match same_name.as_slice() {
            [only @ (_, _, a)] if a.is_none() || address.is_none() => Some(only),
            _ => None,
        };
        exact
            .or(loose)
            .map(|(id, ..)| *id)
            .or_else(|| self.created.get(&(name, address)).copied())
    }
}

async fn stored_shops(db: &impl ConnectionTrait) -> Result<Directory, DbErr> {
    let mut out = Directory::default();
    for row in db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id, name, address FROM shops ORDER BY id",
        ))
        .await?
    {
        let shop = ShopText {
            name: row.try_get_by_index(1)?,
            address: row.try_get_by_index(2)?,
        };
        let (name, address) = directory_key(&shop);
        out.stored.push((row.try_get_by_index(0)?, name, address));
    }
    Ok(out)
}

// The id of the shop `directory` finds for this name and address, or of a
// new one, which `directory` then holds.
async fn shop_for(
    db: &impl ConnectionTrait,
    directory: &mut Directory,
    shop: &ShopText,
    stamp: &Value,
) -> Result<i32, DbErr> {
    if let Some(id) = directory.find(shop) {
        return Ok(id);
    }
    let insert = Query::insert()
        .into_table(Alias::new("shops"))
        .columns(["name", "address", "created_at", "updated_at"].map(Alias::new))
        .values_panic([
            shop.name.clone().into(),
            shop.address.clone().into(),
            stamp.clone().into(),
            stamp.clone().into(),
        ])
        .returning_col(Alias::new("id"))
        .to_owned();
    let row = db
        .query_one(&insert)
        .await?
        .ok_or_else(|| DbErr::Migration("inserting a shop returned no id".into()))?;
    let id = row.try_get_by_index(0)?;
    directory.created.insert(directory_key(shop), id);
    Ok(id)
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One transaction moves every row and adds the checks, so a failure
        // leaves the names where they were and a rerun starts from them.
        let txn = crate::services::begin_write(manager.get_connection()).await?;
        // `m20261124_000001_drop_unused_columns` drops the columns, and a
        // rerun over a database without them has no name to move or to hold
        // empty.
        if !tables_hold_names(&txn).await? {
            return Ok(());
        }
        let stamp = Value::from(crate::services::now());
        // A row that already names a shop keeps it, and a blank name names
        // none, so each of those only loses its text.
        let rows: Vec<(Named, Option<ShopText>)> = named_rows(&txn)
            .await?
            .into_iter()
            .map(|row| {
                let text = row
                    .shop_id
                    .is_none()
                    .then(|| split_name_and_address(&row.text))
                    .flatten();
                (row, text)
            })
            .collect();
        let texts: Vec<ShopText> = rows.iter().filter_map(|(_, t)| t.clone()).collect();
        let mut planned = plan_shops(&texts).into_iter();
        let mut directory = stored_shops(&txn).await?;
        for (row, text) in &rows {
            let shop_id = match text {
                Some(_) => {
                    let shop = planned.next().expect("a planned shop for each name");
                    Some(shop_for(&txn, &mut directory, &shop, &stamp).await?)
                }
                None => row.shop_id,
            };
            let update = Query::update()
                .table(Alias::new(row.table))
                .value(Alias::new("shop_id"), shop_id)
                .value(Alias::new("shop_name"), Option::<String>::None)
                .value(Alias::new("updated_at"), stamp.clone())
                .and_where(Expr::col(Alias::new("id")).eq(row.id))
                .to_owned();
            txn.execute(&update).await?;
        }
        add_checks(&txn, "moving shop names into the shop directory", CHECKS).await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The shops and the checks are what the code that reads `shop_id`
        // needs, and an older build reads a NULL `shop_name` as no shop, so
        // `down` leaves them.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::ConnectionTrait;
    use sea_orm_migration::MigratorTrait;

    use super::{ShopText, plan_shops, split_name_and_address};
    use crate::migration::{
        Migrator,
        schema::testing::{count, fts_integrity, migrated_to, text},
    };

    // Nineteen migrations run before this one.
    const BEFORE: u32 = 19;

    const STAMP: &str = "'2026-11-21T00:00:00+00:00'";

    fn shop(name: &str, address: Option<&str>) -> ShopText {
        ShopText {
            name: name.into(),
            address: address.map(Into::into),
        }
    }

    #[test]
    fn a_trailing_parenthesis_is_the_address() {
        assert_eq!(
            split_name_and_address(" America's Tire (433 E Shaw Ave, Fresno) "),
            Some(shop("America's Tire", Some("433 E Shaw Ave, Fresno")))
        );
        assert_eq!(
            split_name_and_address("Clovis Volkswagen"),
            Some(shop("Clovis Volkswagen", None))
        );
        // Without a name before it or text inside it, a parenthesis is part of
        // the name.
        assert_eq!(
            split_name_and_address("(Fresno)"),
            Some(shop("(Fresno)", None))
        );
        assert_eq!(
            split_name_and_address("Tint ()"),
            Some(shop("Tint ()", None))
        );
        assert_eq!(
            split_name_and_address("Jiffy Lube (oil only)"),
            Some(shop("Jiffy Lube (oil only)", None))
        );
        assert_eq!(split_name_and_address("   "), None);
    }

    #[test]
    fn equal_names_join_unless_their_addresses_differ() {
        let texts = [
            shop("KW Automotive", None),
            shop("kw automotive ", Some("520 N Blackstone, Fresno")),
            shop("America's Tire", Some("433 E Shaw Ave")),
            shop("America's Tire", Some("833 E Shaw Ave")),
            shop("America's Tire", None),
            shop("Volkswagen of Clovis", None),
            shop("Clovis Volkswagen", None),
        ];
        assert_eq!(
            plan_shops(&texts),
            [
                shop("KW Automotive", Some("520 N Blackstone, Fresno")),
                shop("KW Automotive", Some("520 N Blackstone, Fresno")),
                shop("America's Tire", Some("433 E Shaw Ave")),
                shop("America's Tire", Some("833 E Shaw Ave")),
                shop("America's Tire", None),
                shop("Volkswagen of Clovis", None),
                shop("Clovis Volkswagen", None),
            ]
        );
    }

    #[tokio::test]
    async fn every_shop_name_becomes_a_shop_record() {
        let db = migrated_to(BEFORE).await;
        db.execute_unprepared(&format!(
            "INSERT INTO vehicles (id, name, created_at, updated_at)
                 VALUES (1, 'Golf', {STAMP}, {STAMP});
             INSERT INTO shops (id, name, address, created_at, updated_at)
                 VALUES (7, 'Tint Pros', '6129 N Blackstone Ave', {STAMP}, {STAMP});
             INSERT INTO service_records (id, vehicle_id, service_date, shop_name, shop_id,
                     performed_by, created_at, updated_at)
                 VALUES (1, 1, '2026-01-01', 'KW Automotive', NULL, 'shop', {STAMP}, {STAMP}),
                        (2, 1, '2026-02-01', 'KW Automotive (520 N Blackstone, Fresno)', NULL,
                            'shop', {STAMP}, {STAMP}),
                        (3, 1, '2026-03-01', 'Volkswagen of Clovis', NULL, 'shop',
                            {STAMP}, {STAMP}),
                        (4, 1, '2026-04-01', 'Clovis Volkswagen', NULL, 'shop', {STAMP}, {STAMP}),
                        (5, 1, '2026-05-01', 'Tint Pros (6129 N Blackstone Ave)', NULL, 'shop',
                            {STAMP}, {STAMP}),
                        (6, 1, '2026-06-01', 'Somewhere else', 7, 'shop', {STAMP}, {STAMP}),
                        (8, 1, '2026-08-01', 'tint pros', NULL, 'shop', {STAMP}, {STAMP}),
                        (7, 1, '2026-07-01', NULL, NULL, 'self', {STAMP}, {STAMP});
             INSERT INTO appointments (id, vehicle_id, purpose, shop_name, created_at, updated_at)
                 VALUES (1, 1, 'Brakes', 'kw automotive', {STAMP}, {STAMP}),
                        (2, 1, 'Tint', '  ', {STAMP}, {STAMP});"
        ))
        .await
        .unwrap();
        let shops = || {
            text(
                &db,
                "SELECT group_concat(id || ':' || name || ':' || coalesce(address, '-'), ' | ') \
                 FROM (SELECT * FROM shops ORDER BY id)",
            )
        };
        let links = |table: &str| {
            format!(
                "SELECT group_concat(id || '=' || coalesce(shop_id, '-'), ' ') FROM (SELECT * \
                 FROM {table} ORDER BY id)"
            )
        };

        Migrator::up(&db, Some(1)).await.unwrap();
        assert_eq!(
            shops().await,
            "7:Tint Pros:6129 N Blackstone Ave | 8:KW Automotive:520 N Blackstone, Fresno | \
             9:Volkswagen of Clovis:- | 10:Clovis Volkswagen:-"
        );
        assert_eq!(
            text(&db, &links("service_records")).await,
            "1=8 2=8 3=9 4=10 5=7 6=7 7=- 8=7"
        );
        assert_eq!(text(&db, &links("appointments")).await, "1=8 2=-");
        let named = "SELECT (SELECT count(*) FROM service_records WHERE shop_name IS NOT NULL) + \
                     (SELECT count(*) FROM appointments WHERE shop_name IS NOT NULL)";
        assert_eq!(count(&db, named).await, 0);
        fts_integrity(&db, &["fts_service_records", "fts_shops"]).await;

        Migrator::down(&db, Some(1)).await.unwrap();
        Migrator::up(&db, Some(1)).await.unwrap();
        assert_eq!(count(&db, "SELECT count(*) FROM shops").await, 4);

        let refused = db
            .execute_unprepared("UPDATE appointments SET shop_name = 'KW' WHERE id = 1")
            .await;
        assert!(refused.is_err());
    }
}
