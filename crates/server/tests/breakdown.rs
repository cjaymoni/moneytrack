//! Where the money went, over the wire: one breakdown per currency, transfers out, every
//! figure and every share from `mt-core`.

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::TestServer;
use serde_json::{Value, json};

async fn account(server: &TestServer, name: &str, currency: &str) -> String {
    let body: Value = server
        .post(
            "/accounts",
            json!({ "name": name, "kind": "momo", "currency": currency }),
        )
        .await
        .json()
        .await
        .unwrap();
    body["id"].as_str().unwrap().to_owned()
}

async fn category(server: &TestServer, name: &str) -> String {
    let list: Value = server.get("/categories").await.json().await.unwrap();
    list.as_array()
        .unwrap()
        .iter()
        .find(|category| category["name"] == name)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn record(server: &TestServer, body: Value) {
    let response = server.post("/transactions", body).await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
}

#[tokio::test]
async fn a_month_is_grouped_by_category_with_shares_from_the_core() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;
    let wallet = account(&server, "Wallet", "GHS").await;
    let food = category(&server, "Food & groceries").await;
    let transport = category(&server, "Transport").await;
    let salary = category(&server, "Salary").await;

    for (direction, amount, category, at) in [
        ("out", "150", Some(&food), "2026-11-03T10:00:00Z"),
        ("out", "50", Some(&transport), "2026-11-04T10:00:00Z"),
        ("out", "25", None, "2026-11-05T10:00:00Z"),
        ("in", "1000", Some(&salary), "2026-11-01T10:00:00Z"),
        // October: not this month.
        ("out", "999", Some(&food), "2026-10-31T10:00:00Z"),
    ] {
        let mut body = json!({ "account_id": wallet, "direction": direction, "amount": amount,
                               "label": "Row", "booked_at": at });
        if let Some(category) = category {
            body["category_id"] = json!(category);
        }
        record(&server, body).await;
    }

    let response = server.get("/breakdown?month=2026-11").await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let all: Value = response.json().await.unwrap();

    assert_eq!(all.as_array().unwrap().len(), 1);
    let month = &all[0];
    assert_eq!(month["currency"], "GHS");
    assert_eq!(month["starts_on"], "2026-11-01");
    assert_eq!(month["ends_on"], "2026-11-30");
    assert_eq!(month["spending"]["total"]["display"], "225.00");
    assert_eq!(month["income"]["total"]["display"], "1000.00");
    assert_eq!(month["net"]["display"], "775.00");

    let lines = month["spending"]["lines"].as_array().unwrap();
    assert_eq!(lines[0]["name"], "Food & groceries");
    assert_eq!(lines[0]["share"]["basis_points"], 6_666);
    assert_eq!(lines[0]["share"]["display"], "66%");
    assert_eq!(lines[1]["name"], "Transport");
    assert!(lines[2]["category_id"].is_null());
    assert!(lines[2]["name"].is_null());
    assert_eq!(lines[2]["total"]["display"], "25.00");
}

#[tokio::test]
async fn each_currency_gets_its_own_breakdown_and_none_is_converted() {
    let server = TestServer::start().await;
    server.register("kofi@example.test").await;
    let cedis = account(&server, "Wallet", "GHS").await;
    let francs = account(&server, "Lomé", "XOF").await;

    record(
        &server,
        json!({ "account_id": cedis, "direction": "out", "amount": "10", "label": "Kenkey",
                "booked_at": "2026-11-02T10:00:00Z" }),
    )
    .await;
    record(
        &server,
        json!({ "account_id": francs, "direction": "out", "amount": "500", "label": "Taxi",
                "booked_at": "2026-11-02T10:00:00Z" }),
    )
    .await;

    let all: Value = server
        .get("/breakdown?month=2026-11")
        .await
        .json()
        .await
        .unwrap();

    let all = all.as_array().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0]["currency"], "GHS");
    assert_eq!(all[0]["spending"]["total"]["display"], "10.00");
    assert_eq!(all[1]["currency"], "XOF");
    assert_eq!(all[1]["spending"]["total"]["display"], "500");
}

#[tokio::test]
async fn the_readers_offset_decides_which_month_a_late_row_is_in() {
    let server = TestServer::start().await;
    server.register("esi@example.test").await;
    let wallet = account(&server, "Wallet", "GHS").await;
    record(
        &server,
        json!({ "account_id": wallet, "direction": "out", "amount": "10", "label": "Late",
                "booked_at": "2026-11-01T02:30:00Z" }),
    )
    .await;

    let in_utc: Value = server
        .get("/breakdown?month=2026-11")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(in_utc[0]["spending"]["total"]["display"], "10.00");

    // Three hours behind UTC, it was still 31 October on the reader's clock.
    let behind: Value = server
        .get("/breakdown?month=2026-11&offset_minutes=-180")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(behind[0]["spending"]["total"]["display"], "0.00");
}

#[tokio::test]
async fn a_month_or_offset_that_will_not_read_names_the_field() {
    let server = TestServer::start().await;
    server.register("yaw@example.test").await;

    for (query, field) in [
        ("month=2026-13", "month"),
        ("month=November", "month"),
        ("month=2026-11&offset_minutes=9999", "offset_minutes"),
    ] {
        let response = server.get(&format!("/breakdown?{query}")).await;
        assert_eq!(response.status(), 422, "{query}");
        let body: Value = response.json().await.unwrap();
        assert!(
            body["message"].as_str().unwrap().contains(field),
            "{query}: {body}"
        );
    }
}

#[tokio::test]
async fn nobody_in_particular_gets_no_breakdown() {
    let server = TestServer::start().await;
    assert_eq!(server.get("/breakdown?month=2026-11").await.status(), 401);
}
