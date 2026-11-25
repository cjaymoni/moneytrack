//! Month by month, over the wire: one trend per currency, every figure and every height on
//! the shared scale from `mt-core`.

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

async fn record(server: &TestServer, account: &str, direction: &str, amount: &str, at: &str) {
    let response = server
        .post(
            "/transactions",
            json!({ "account_id": account, "direction": direction, "amount": amount,
                    "label": "Row", "booked_at": at }),
        )
        .await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
}

#[tokio::test]
async fn months_come_oldest_first_with_heights_on_one_scale() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;
    let wallet = account(&server, "Wallet", "GHS").await;
    record(&server, &wallet, "in", "6300", "2026-10-25T10:00:00Z").await;
    record(&server, &wallet, "out", "3150", "2026-11-02T10:00:00Z").await;
    // Before the window: not counted.
    record(&server, &wallet, "out", "999", "2026-08-31T10:00:00Z").await;

    let response = server.get("/trend?until=2026-11&months=3").await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let all: Value = response.json().await.unwrap();

    assert_eq!(all.as_array().unwrap().len(), 1);
    let trend = &all[0];
    assert_eq!(trend["currency"], "GHS");
    let months: Vec<&str> = trend["months"]
        .as_array()
        .unwrap()
        .iter()
        .map(|month| month["month"].as_str().unwrap())
        .collect();
    assert_eq!(months, ["2026-09", "2026-10", "2026-11"]);

    let september = &trend["months"][0];
    assert_eq!(september["spending"]["display"], "0.00");
    assert_eq!(september["spending_level_bps"], 0);
    let october = &trend["months"][1];
    assert_eq!(october["starts_on"], "2026-10-01");
    assert_eq!(october["ends_on"], "2026-10-31");
    assert_eq!(october["income"]["display"], "6300.00");
    assert_eq!(october["income_level_bps"], 7_875);
    let november = &trend["months"][2];
    assert_eq!(november["spending"]["display"], "3150.00");
    assert_eq!(november["net"]["display"], "-3150.00");
    assert_eq!(november["spending_level_bps"], 3_937);

    let ticks: Vec<(&str, u64)> = trend["ticks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tick| {
            (
                tick["amount"]["display"].as_str().unwrap(),
                tick["level_bps"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        ticks,
        [
            ("0.00", 0),
            ("2000.00", 2_500),
            ("4000.00", 5_000),
            ("6000.00", 7_500),
            ("8000.00", 10_000)
        ]
    );
}

#[tokio::test]
async fn each_currency_gets_its_own_trend_and_six_months_by_default() {
    let server = TestServer::start().await;
    server.register("kofi@example.test").await;
    let cedis = account(&server, "Wallet", "GHS").await;
    let francs = account(&server, "Lomé", "XOF").await;
    record(&server, &cedis, "out", "10", "2026-11-02T10:00:00Z").await;
    record(&server, &francs, "out", "500", "2026-11-02T10:00:00Z").await;

    let all: Value = server
        .get("/trend?until=2026-11")
        .await
        .json()
        .await
        .unwrap();

    let all = all.as_array().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0]["currency"], "GHS");
    assert_eq!(all[0]["months"].as_array().unwrap().len(), 6);
    assert_eq!(all[0]["months"][5]["spending"]["display"], "10.00");
    assert_eq!(all[1]["currency"], "XOF");
    assert_eq!(all[1]["months"][5]["spending"]["display"], "500");
}

#[tokio::test]
async fn a_month_count_or_offset_that_will_not_read_names_the_field() {
    let server = TestServer::start().await;
    server.register("yaw@example.test").await;

    for (query, field) in [
        ("until=2026-13", "until"),
        ("until=November", "until"),
        ("until=2026-11&months=0", "months"),
        ("until=2026-11&months=25", "months"),
        ("until=2026-11&offset_minutes=9999", "offset_minutes"),
    ] {
        let response = server.get(&format!("/trend?{query}")).await;
        assert_eq!(response.status(), 422, "{query}");
        let body: Value = response.json().await.unwrap();
        assert!(
            body["message"].as_str().unwrap().contains(field),
            "{query}: {body}"
        );
    }
}

#[tokio::test]
async fn each_persons_trend_is_their_own() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;
    let wallet = account(&server, "Wallet", "GHS").await;
    record(&server, &wallet, "out", "40", "2026-11-02T10:00:00Z").await;

    let esi = server.another_browser();
    esi.register("esi@example.test").await;
    let mine = account(&esi, "Purse", "GHS").await;
    record(&esi, &mine, "out", "5", "2026-11-03T10:00:00Z").await;

    let all: Value = esi
        .get("/trend?until=2026-11&months=1")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(all[0]["months"][0]["spending"]["display"], "5.00");
}

#[tokio::test]
async fn nobody_in_particular_gets_no_trend() {
    let server = TestServer::start().await;
    assert_eq!(server.get("/trend?until=2026-11").await.status(), 401);
}

#[tokio::test]
async fn a_month_splits_into_days_with_heights_on_one_scale() {
    let server = TestServer::start().await;
    server.register("abena@example.test").await;
    let wallet = account(&server, "Wallet", "GHS").await;
    record(&server, &wallet, "out", "20", "2026-11-03T08:00:00Z").await;
    record(&server, &wallet, "out", "5", "2026-11-03T21:00:00Z").await;
    record(&server, &wallet, "out", "120", "2026-11-30T09:00:00Z").await;
    record(&server, &wallet, "in", "6300", "2026-11-03T09:00:00Z").await;

    let response = server.get("/daily?month=2026-11").await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let all: Value = response.json().await.unwrap();

    let month = &all[0];
    assert_eq!(month["currency"], "GHS");
    assert_eq!(month["total"]["display"], "145.00");
    let days = month["days"].as_array().unwrap();
    assert_eq!(days.len(), 30);
    assert_eq!(days[0]["date"], "2026-11-01");
    assert_eq!(days[0]["spending"]["display"], "0.00");
    assert_eq!(days[2]["spending"]["display"], "25.00");
    assert_eq!(days[29]["spending"]["display"], "120.00");
    assert_eq!(days[29]["level_bps"], 8_000);
    assert_eq!(month["ticks"].as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn a_daily_month_or_offset_that_will_not_read_names_the_field() {
    let server = TestServer::start().await;
    server.register("kwame@example.test").await;
    for (query, field) in [
        ("month=2026-13", "month"),
        ("month=2026-11&offset_minutes=9999", "offset_minutes"),
    ] {
        let response = server.get(&format!("/daily?{query}")).await;
        assert_eq!(response.status(), 422, "{query}");
        let body: Value = response.json().await.unwrap();
        assert!(
            body["message"].as_str().unwrap().contains(field),
            "{query}: {body}"
        );
    }
    let nobody = TestServer::start().await;
    assert_eq!(nobody.get("/daily?month=2026-11").await.status(), 401);
}
