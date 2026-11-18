//! Adds `part_contents`: the items inside a part bought as a kit, each with
//! its own name, manufacturer, number and quantity, in the order they print.

use sea_orm_migration::prelude::*;

use super::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("part_contents"))
                    .if_not_exists()
                    .col(pk())
                    .col(int_not_null("part_id"))
                    .col(int_not_null("position"))
                    .col(text_not_null("name"))
                    .col(text("manufacturer"))
                    .col(text("part_number"))
                    .col(c("quantity").integer().not_null().default(1))
                    .col(text_not_null("created_at"))
                    .col(text_not_null("updated_at"))
                    .foreign_key(&mut fk(
                        "part_contents",
                        "part_id",
                        "parts",
                        ForeignKeyAction::Cascade,
                    ))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(index(
                "idx_part_contents_part",
                "part_contents",
                &["part_id"],
            ))
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("part_contents"))
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}
