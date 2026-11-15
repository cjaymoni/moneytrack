//! Accounts, transactions, and the numbers the browser renders.
//!
//! Every number asserted here was computed by `mt-core` — the same crate a handset runs
//! locally. That is the property these tests are really about: a thin client and an offline
//! one cannot disagree, because only one of them does arithmetic.

#![forbid(unsafe_code)]
// Indexing a `serde_json::Value` by key is how a test says what it expects of a body;
// `.get("x").unwrap()` at every step would bury the assertion.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::TestServer;
use serde_json::{Value, json};

/// Opens a cedi wallet and returns its id.
async fn wallet(server: &TestServer, opening_minor: i64) -> String {
    let response = server
        .post(
            "/accounts",
            json!({ "name": "Wallet", "kind": "momo", "currency": "GHS",
                    "opening_balance_minor": opening_minor }),
        )
        .await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let body: Value = response.json().await.unwrap();
    body["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn an_account_comes_back_with_what_it_holds() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;

    wallet(&server, 25_000).await;
    let accounts: Value = server.get("/accounts").await.json().await.unwrap();

    assert_eq!(accounts.as_array().unwrap().len(), 1);
    let account = &accounts[0];
    assert_eq!(account["name"], "Wallet");
    assert_eq!(account["currency"], "GHS");
    assert_eq!(account["confirmed"]["minor_units"], 25_000);
    // Rendered by mt-core, at the cedi's own two decimal places. A client that divided by a
    // hundred would be right here and wrong for the CFA franc.
    assert_eq!(account["confirmed"]["display"], "250.00");
}

#[tokio::test]
async fn a_transaction_moves_the_balance() {
    let server = TestServer::start().await;
    server.register("kofi@example.test").await;
    let account = wallet(&server, 25_000).await;

    let written = server
        .post(
            "/transactions",
            json!({ "account_id": account, "direction": "out", "amount_minor": 4_050,
                    "label": "Trotro" }),
        )
        .await;
    assert_eq!(written.status(), 200, "{}", written.text().await.unwrap());

    let accounts: Value = server.get("/accounts").await.json().await.unwrap();
    assert_eq!(accounts[0]["confirmed"]["minor_units"], 20_950);
    assert_eq!(accounts[0]["confirmed"]["display"], "209.50");
}

#[tokio::test]
async fn a_pending_row_is_the_forecast_and_not_the_balance() {
    let server = TestServer::start().await;
    server.register("esi@example.test").await;
    let account = wallet(&server, 25_000).await;

    server
        .post(
            "/transactions",
            json!({ "account_id": account, "direction": "out", "amount_minor": 90_000,
                    "label": "Rent", "booked_at": "2027-01-01T00:00:00Z",
                    "status": "pending" }),
        )
        .await;

    let accounts: Value = server.get("/accounts").await.json().await.unwrap();
    assert_eq!(accounts[0]["confirmed"]["minor_units"], 25_000);
    assert_eq!(accounts[0]["projected"]["minor_units"], -65_000);
}

#[tokio::test]
async fn a_row_reads_back_in_the_wire_spellings() {
    let server = TestServer::start().await;
    server.register("yaa@example.test").await;
    let account = wallet(&server, 0).await;

    server
        .post(
            "/transactions",
            json!({ "account_id": account, "direction": "out", "amount_minor": 1_234,
                    "label": "Kɔkɔɔ — ₵ fare 🚐", "counterparty": "Akosua" }),
        )
        .await;
    let rows: Value = server.get("/transactions").await.json().await.unwrap();

    let row = &rows[0];
    assert_eq!(row["direction"], "out");
    assert_eq!(row["status"], "confirmed");
    assert_eq!(row["source"], "manual");
    assert_eq!(row["amount"]["minor_units"], 1_234);
    assert_eq!(row["amount"]["display"], "12.34");
    assert_eq!(row["label"], "Kɔkɔɔ — ₵ fare 🚐");
    assert_eq!(row["counterparty"], "Akosua");
    assert!(row["booked_at"].as_str().unwrap().ends_with('Z'));
}

#[tokio::test]
async fn totals_are_per_currency_and_never_converted() {
    let server = TestServer::start().await;
    server.register("sena@example.test").await;
    wallet(&server, 25_000).await;
    server
        .post(
            "/accounts",
            json!({ "name": "Dollars", "kind": "bank", "currency": "USD",
                    "opening_balance_minor": 10_000 }),
        )
        .await;

    let totals: Value = server.get("/totals").await.json().await.unwrap();

    let totals = totals.as_array().unwrap();
    assert_eq!(totals.len(), 2, "two currencies became {totals:?}");
    assert!(
        totals
            .iter()
            .any(|total| total["currency"] == "GHS" && total["minor_units"] == 25_000)
    );
    assert!(
        totals
            .iter()
            .any(|total| total["currency"] == "USD" && total["minor_units"] == 10_000)
    );
}

#[tokio::test]
async fn a_currency_the_registry_does_not_know_is_refused() {
    let server = TestServer::start().await;
    server.register("adjoa@example.test").await;

    let response = server
        .post(
            "/accounts",
            json!({ "name": "Wallet", "kind": "momo", "currency": "XYZ",
                    "opening_balance_minor": 0 }),
        )
        .await;

    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "invalid");
}

#[tokio::test]
async fn an_amount_past_the_ceiling_is_refused_rather_than_stored() {
    let server = TestServer::start().await;
    server.register("nii@example.test").await;
    let account = wallet(&server, 0).await;

    let response = server
        .post(
            "/transactions",
            json!({ "account_id": account, "direction": "out",
                    "amount_minor": 9_000_000_000_000_000i64, "label": "Too much" }),
        )
        .await;

    assert_eq!(response.status(), 422);
}

#[tokio::test]
async fn an_id_that_is_not_a_uuid_names_the_field() {
    let server = TestServer::start().await;
    server.register("kwesi@example.test").await;

    let response = server
        .post(
            "/transactions",
            json!({ "account_id": "wallet-1", "direction": "out", "amount_minor": 100,
                    "label": "Trotro" }),
        )
        .await;

    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "invalid");
    assert!(
        body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("account_id"),
        "the refusal does not name the field: {body}"
    );
}
