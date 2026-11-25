//! Counting an account and making the ledger agree: one correcting row, from the adjustment
//! source, worked out by `mt-core`.

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

async fn wallet(server: &TestServer, opening: &str) -> String {
    let body: Value = server
        .post(
            "/accounts",
            json!({ "name": "Cash", "kind": "cash", "currency": "GHS",
                    "opening_balance": opening }),
        )
        .await
        .json()
        .await
        .unwrap();
    body["id"].as_str().unwrap().to_owned()
}

async fn count(server: &TestServer, account: &str, counted: &str) -> reqwest::Response {
    server
        .post(
            &format!("/accounts/{account}/reconcile"),
            json!({ "counted": counted }),
        )
        .await
}

async fn held(server: &TestServer, account: &str) -> String {
    let ledger: Value = server
        .get(&format!("/ledger?account_id={account}&basis=confirmed"))
        .await
        .json()
        .await
        .unwrap();
    ledger["closing"]["display"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn a_count_short_of_the_ledger_writes_one_row_out_and_the_ledger_agrees() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;
    let cash = wallet(&server, "380").await;

    let response = count(&server, &cash, "340").await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let answer: Value = response.json().await.unwrap();

    assert_eq!(answer["ledger"]["display"], "380.00");
    assert_eq!(answer["counted"]["display"], "340.00");
    let row = &answer["adjustment"];
    assert_eq!(row["direction"], "out");
    assert_eq!(row["amount"]["display"], "40.00");
    assert_eq!(row["source"], "adjustment");
    assert_eq!(row["status"], "confirmed");
    assert_eq!(row["label"], "Balance updated to 340.00 GHS");
    assert_eq!(held(&server, &cash).await, "340.00");
}

#[tokio::test]
async fn a_count_over_the_ledger_writes_money_in() {
    let server = TestServer::start().await;
    server.register("kofi@example.test").await;
    let cash = wallet(&server, "100").await;

    let answer: Value = count(&server, &cash, "105.50").await.json().await.unwrap();

    assert_eq!(answer["adjustment"]["direction"], "in");
    assert_eq!(answer["adjustment"]["amount"]["display"], "5.50");
    assert_eq!(held(&server, &cash).await, "105.50");
}

#[tokio::test]
async fn a_count_that_agrees_writes_nothing() {
    let server = TestServer::start().await;
    server.register("esi@example.test").await;
    let cash = wallet(&server, "100").await;

    let answer: Value = count(&server, &cash, "100").await.json().await.unwrap();

    assert!(answer["adjustment"].is_null());
    let rows: Value = server.get("/transactions").await.json().await.unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn expected_rows_are_not_what_a_count_measures() {
    let server = TestServer::start().await;
    server.register("yaw@example.test").await;
    let cash = wallet(&server, "100").await;
    // Expected and dated yesterday, never confirmed: the money has not actually gone.
    let yesterday = (time::OffsetDateTime::now_utc() - time::Duration::days(1))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    server
        .post(
            "/transactions",
            json!({ "account_id": cash, "direction": "out", "amount": "30",
                    "label": "Bill", "booked_at": yesterday, "status": "pending" }),
        )
        .await;

    let answer: Value = count(&server, &cash, "100").await.json().await.unwrap();

    assert_eq!(answer["ledger"]["display"], "100.00");
    assert!(answer["adjustment"].is_null());
}

#[tokio::test]
async fn a_count_that_will_not_read_names_the_field_and_writes_nothing() {
    let server = TestServer::start().await;
    server.register("abena@example.test").await;
    let cash = wallet(&server, "100").await;

    let response = count(&server, &cash, "1.005").await;

    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["message"].as_str().unwrap().contains("counted"),
        "{body}"
    );
    assert_eq!(held(&server, &cash).await, "100.00");
}

#[tokio::test]
async fn an_adjustment_can_be_deleted_like_any_row() {
    let server = TestServer::start().await;
    server.register("akos@example.test").await;
    let cash = wallet(&server, "100").await;
    let answer: Value = count(&server, &cash, "90").await.json().await.unwrap();
    let id = answer["adjustment"]["id"].as_str().unwrap();

    assert_eq!(
        server.delete(&format!("/transactions/{id}")).await.status(),
        200
    );
    assert_eq!(held(&server, &cash).await, "100.00");
}

#[tokio::test]
async fn nobody_can_count_anothers_account() {
    let ama = TestServer::start().await;
    ama.register("ama@example.test").await;
    let cash = wallet(&ama, "100").await;
    let kofi = ama.another_browser();
    kofi.register("kofi@example.test").await;

    let response = count(&kofi, &cash, "0").await;

    assert_eq!(response.status(), 404);
    assert_eq!(held(&ama, &cash).await, "100.00");
}
