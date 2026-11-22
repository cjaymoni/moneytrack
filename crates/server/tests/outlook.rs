//! The outlook over the wire: one per account, the shortfall named by its row, and levels
//! the browser can draw without doing arithmetic.

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
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

/// An instant `days` from now, as the wire spells it.
fn in_days(days: i64) -> String {
    (OffsetDateTime::now_utc() + Duration::days(days))
        .format(&Rfc3339)
        .unwrap()
}

async fn account(server: &TestServer, name: &str, opening: &str) -> String {
    let body: Value = server
        .post(
            "/accounts",
            json!({ "name": name, "kind": "momo", "currency": "GHS",
                    "opening_balance": opening }),
        )
        .await
        .json()
        .await
        .unwrap();
    body["id"].as_str().unwrap().to_owned()
}

async fn expect(
    server: &TestServer,
    account: &str,
    direction: &str,
    amount: &str,
    days: i64,
    label: &str,
) -> String {
    let body: Value = server
        .post(
            "/transactions",
            json!({ "account_id": account, "direction": direction, "amount": amount,
                    "label": label, "booked_at": in_days(days), "status": "pending" }),
        )
        .await
        .json()
        .await
        .unwrap();
    body["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn the_row_that_runs_an_account_short_is_named() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;
    let wallet = account(&server, "Wallet", "500").await;
    expect(&server, &wallet, "out", "300", 5, "School fees").await;
    let rent = expect(&server, &wallet, "out", "400", 10, "Rent").await;
    expect(&server, &wallet, "in", "1000", 20, "Salary").await;

    let response = server.get("/outlook").await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let all: Value = response.json().await.unwrap();

    let ahead = &all[0];
    assert_eq!(ahead["account"]["name"], "Wallet");
    assert_eq!(ahead["start"]["display"], "500.00");
    assert_eq!(ahead["shortfall"], "at");
    assert_eq!(ahead["shortfall_transaction_id"], rent);
    assert_eq!(ahead["low"]["display"], "-200.00");
    assert_eq!(ahead["low_transaction_id"], rent);
    assert_eq!(ahead["high"]["display"], "800.00");

    let walked: Vec<(&str, &str, u64)> = ahead["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|point| {
            (
                point["transaction"]["label"].as_str().unwrap(),
                point["balance_after"]["display"].as_str().unwrap(),
                point["level_bps"].as_u64().unwrap(),
            )
        })
        .collect();
    // From −200 to 800: a range of 1 000, so 200 is a fifth of the way up.
    assert_eq!(
        walked,
        [
            ("School fees", "200.00", 4_000),
            ("Rent", "-200.00", 0),
            ("Salary", "800.00", 10_000)
        ]
    );
    assert_eq!(ahead["start_level_bps"], 7_000);
    assert_eq!(ahead["zero_level_bps"], 2_000);
}

#[tokio::test]
async fn each_account_is_its_own_outlook_and_a_surplus_elsewhere_covers_nothing() {
    let server = TestServer::start().await;
    server.register("kofi@example.test").await;
    let wallet = account(&server, "Wallet", "50").await;
    account(&server, "Bank", "10000").await;
    expect(&server, &wallet, "out", "80", 3, "Light bill").await;

    let all: Value = server.get("/outlook").await.json().await.unwrap();

    let all = all.as_array().unwrap();
    assert_eq!(all.len(), 2);
    let wallet_ahead = all
        .iter()
        .find(|each| each["account"]["name"] == "Wallet")
        .unwrap();
    let bank_ahead = all
        .iter()
        .find(|each| each["account"]["name"] == "Bank")
        .unwrap();
    assert_eq!(wallet_ahead["shortfall"], "at");
    assert_eq!(bank_ahead["shortfall"], "none");
    assert!(bank_ahead["low_transaction_id"].is_null());
    // A 10 000.00 balance going nowhere sits at the top, with zero at the bottom.
    assert_eq!(bank_ahead["start_level_bps"], 10_000);
    assert_eq!(bank_ahead["zero_level_bps"], 0);
}

#[tokio::test]
async fn an_account_already_short_says_so() {
    let server = TestServer::start().await;
    server.register("esi@example.test").await;
    let wallet = account(&server, "Wallet", "10").await;
    // Expected two days ago and never confirmed: the forecast counts it.
    expect(&server, &wallet, "out", "30", -2, "Overdue bill").await;

    let all: Value = server.get("/outlook").await.json().await.unwrap();

    assert_eq!(all[0]["start"]["display"], "-20.00");
    assert_eq!(all[0]["shortfall"], "already");
    assert!(all[0]["shortfall_transaction_id"].is_null());
}

#[tokio::test]
async fn the_window_is_bounded_and_says_what_it_wants() {
    let server = TestServer::start().await;
    server.register("yaw@example.test").await;
    let wallet = account(&server, "Wallet", "100").await;
    expect(&server, &wallet, "out", "500", 60, "In two months").await;

    let month: Value = server.get("/outlook?days=30").await.json().await.unwrap();
    assert_eq!(month[0]["points"].as_array().unwrap().len(), 0);
    assert_eq!(month[0]["shortfall"], "none");

    let quarter: Value = server.get("/outlook?days=90").await.json().await.unwrap();
    assert_eq!(quarter[0]["shortfall"], "at");

    for bad in ["0", "400", "-5"] {
        let response = server.get(&format!("/outlook?days={bad}")).await;
        assert_eq!(response.status(), 422, "days={bad}");
        let body: Value = response.json().await.unwrap();
        assert!(body["message"].as_str().unwrap().contains("days"), "{body}");
    }
}

#[tokio::test]
async fn nobody_sees_anothers_outlook() {
    let ama = TestServer::start().await;
    ama.register("ama@example.test").await;
    let wallet = account(&ama, "Ama's wallet", "100").await;
    expect(&ama, &wallet, "out", "500", 5, "Rent").await;
    let kofi = ama.another_browser();
    kofi.register("kofi@example.test").await;

    let theirs: Value = kofi.get("/outlook").await.json().await.unwrap();
    assert_eq!(theirs.as_array().unwrap().len(), 0, "{theirs}");

    let signed_out = ama.another_browser();
    assert_eq!(signed_out.get("/outlook").await.status(), 401);
}
