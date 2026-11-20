//! Moves every free-text shop name on services and appointments into the shop
//! directory: each row then names a shop by `shop_id` and its `shop_name` is
//! empty, and a CHECK keeps it empty.
//!
//! A trailing `(…)` in a stored name is the shop's address. Names that are
//! equal apart from ASCII case and surrounding spaces become one shop unless
//! their addresses differ, and then each address is a shop of its own. Names
//! that differ, such as one dealer under two names, stay two shops for the
//! owner to merge.

use std::collections::HashMap;

use sea_orm::{DatabaseBackend, Statement, Value};
use sea_orm_migration::prelude::*;

use super::schema::{Check, add_checks};

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
pub(super) struct ShopText {
    pub name: String,
    pub address: Option<String>,
}

pub(super) fn split_name_and_address(text: &str) -> Option<ShopText> {
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
        .filter(|(name, address)| !name.is_empty() && !address.is_empty());
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

fn key(text: &str) -> String {
    text.trim().to_ascii_lowercase()
}

/// The shop each stored name becomes, in the order the names are given.
/// Names that share a key share a shop, which takes the first spelling and
/// the group's address, unless the group holds two addresses: then each
/// address is its own shop and the entries with none share one more.
pub(super) fn plan_shops(texts: &[ShopText]) -> Vec<ShopText> {
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

// The id of a stored shop with this name and address, compared by key, or a
// new shop's.
async fn shop_for(db: &impl ConnectionTrait, shop: &ShopText, stamp: &Value) -> Result<i32, DbErr> {
    for row in db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id, name, address FROM shops ORDER BY id",
        ))
        .await?
    {
        let name: String = row.try_get_by_index(1)?;
        let address: Option<String> = row.try_get_by_index(2)?;
        if key(&name) == key(&shop.name)
            && address.as_deref().map(key) == shop.address.as_deref().map(key)
        {
            return row.try_get_by_index(0);
        }
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
    row.try_get_by_index(0)
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One transaction moves every row and adds the checks, so a failure
        // leaves the names where they were and a rerun starts from them.
        let txn = crate::services::begin_write(manager.get_connection()).await?;
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
        for (row, text) in &rows {
            let shop_id = match text {
                Some(_) => {
                    let shop = planned.next().expect("a planned shop for each name");
                    Some(shop_for(&txn, &shop, &stamp).await?)
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
            "1=8 2=8 3=9 4=10 5=7 6=7 7=-"
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
