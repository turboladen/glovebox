mod common;

use common::{add_vehicle, assert_unknown_field_rejected, connect, id, minimal_service};
use glovebox_shared::{
    inputs::{service_record::NewServiceRecord, shop::ShopChoice},
    services::service_record,
};
use serde_json::json;

#[tokio::test]
async fn a_shop_is_added_listed_corrected_and_deleted() {
    let t = connect().await;
    let added = t
        .call(
            "add_shop",
            json!({ "name": "KW Automotive", "address": "520 N Blackstone", "specialty": "VW" }),
        )
        .await;
    let shop_id = id(&added);
    assert_eq!(added["name"], "KW Automotive");

    let err = t
        .call_err("add_shop", json!({ "name": "kw automotive" }))
        .await;
    assert!(err.contains(&format!("Shop {shop_id}")), "{err}");

    let listed = t.call("list_shops", json!({})).await;
    assert_eq!(listed[0]["id"], shop_id);
    assert_eq!(listed[0]["service_count"], 0);

    let updated = t
        .call(
            "update_shop",
            json!({ "shop_id": shop_id, "phone": "555-0100", "specialty": null }),
        )
        .await;
    assert_eq!(updated["phone"], "555-0100");
    assert_eq!(updated["specialty"], serde_json::Value::Null);
    assert_eq!(updated["address"], "520 N Blackstone");

    let deleted = t.call("delete_shop", json!({ "shop_id": shop_id })).await;
    assert_eq!(deleted, json!({ "deleted": "shop", "id": shop_id }));
    let err = t.call_err("get_shop", json!({ "shop_id": shop_id })).await;
    assert_eq!(err, format!("Shop {shop_id} not found"));
}

#[tokio::test]
async fn merging_two_names_of_one_dealer_gives_one_history() {
    let t = connect().await;
    let vid = i32::try_from(add_vehicle(&t, "Golf").await).unwrap();
    let kept = id(&t
        .call("add_shop", json!({ "name": "Clovis Volkswagen" }))
        .await);
    let dup = id(&t
        .call(
            "add_shop",
            json!({ "name": "Volkswagen of Clovis", "phone": "555-0199" }),
        )
        .await);
    for (shop, on) in [(kept, "2025-01-02"), (dup, "2025-06-30")] {
        service_record::create(
            &t.db,
            vid,
            NewServiceRecord {
                shop: Some(ShopChoice::Id(i32::try_from(shop).unwrap())),
                total_cost_cents: Some(10_000),
                ..minimal_service(on, "Oil change")
            },
        )
        .await
        .unwrap();
    }

    let err = t.call_err("delete_shop", json!({ "shop_id": dup })).await;
    assert!(err.contains("Merge it"), "{err}");

    let merged = t
        .call(
            "merge_shops",
            json!({ "keep_shop_id": kept, "merge_shop_ids": [dup] }),
        )
        .await;
    assert_eq!(merged["shop"]["phone"], "555-0199");
    assert_eq!(merged["merged_shop_ids"], json!([dup]));
    assert_eq!(merged["moved_service_ids"].as_array().unwrap().len(), 1);

    let history = t.call("get_shop", json!({ "shop_id": kept })).await;
    assert_eq!(history["name"], "Clovis Volkswagen");
    assert_eq!(history["total_cost_cents"], 20_000);
    assert_eq!(history["services"][0]["service_date"], "2025-06-30");
    assert_eq!(history["services"][0]["vehicle_name"], "Golf");
    assert_eq!(
        t.call("list_shops", json!({}))
            .await
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn the_shop_tools_refuse_an_argument_they_do_not_define() {
    let t = connect().await;
    let cases = [
        ("list_shops", json!({})),
        ("add_shop", json!({ "name": "x" })),
        ("get_shop", json!({ "shop_id": 1 })),
        ("update_shop", json!({ "shop_id": 1 })),
        (
            "merge_shops",
            json!({ "keep_shop_id": 1, "merge_shop_ids": [2] }),
        ),
        ("delete_shop", json!({ "shop_id": 1 })),
    ];
    for (tool, mut args) in cases {
        args["bogus"] = json!(true);
        assert_unknown_field_rejected(&t, tool, args, "bogus").await;
    }
}
