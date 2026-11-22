//! Changing and deleting rows: the balance follows, a deletion is a tombstone that can be
//! undone, and nothing reaches across to another person.

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

/// A cedi wallet holding 100.00, with one row of 12.50 out. Returns (account, row).
async fn wallet_with_a_row(server: &TestServer) -> (String, String) {
    let account: Value = server
        .post(
            "/accounts",
            json!({ "name": "Wallet", "kind": "momo", "currency": "GHS",
                    "opening_balance": "100" }),
        )
        .await
        .json()
        .await
        .unwrap();
    let account = account["id"].as_str().unwrap().to_owned();
    let row: Value = server
        .post(
            "/transactions",
            json!({ "account_id": account, "direction": "out", "amount": "12.50",
                    "label": "Waakye", "counterparty": "Auntie Muni",
                    "booked_at": "2026-11-10T12:00:00Z" }),
        )
        .await
        .json()
        .await
        .unwrap();
    (account, row["id"].as_str().unwrap().to_owned())
}

async fn closing(server: &TestServer, account: &str) -> String {
    let ledger: Value = server
        .get(&format!("/ledger?account_id={account}"))
        .await
        .json()
        .await
        .unwrap();
    ledger["closing"]["display"].as_str().unwrap().to_owned()
}

fn whole(amount: &str, label: &str) -> Value {
    json!({ "direction": "out", "amount": amount, "booked_at": "2026-11-10T12:00:00Z",
            "label": label, "counterparty": "Auntie Muni", "note": null,
            "status": "confirmed", "category_id": null })
}

#[tokio::test]
async fn a_changed_amount_moves_the_balance_and_reads_back() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;
    let (account, row) = wallet_with_a_row(&server).await;
    assert_eq!(closing(&server, &account).await, "87.50");

    let response = server
        .patch(
            &format!("/transactions/{row}"),
            whole("15", "Waakye and egg"),
        )
        .await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let changed: Value = response.json().await.unwrap();

    assert_eq!(changed["amount"]["display"], "15.00");
    assert_eq!(changed["label"], "Waakye and egg");
    assert_eq!(changed["source"], "manual");
    assert_eq!(closing(&server, &account).await, "85.00");
}

#[tokio::test]
async fn an_edit_replaces_the_whole_row_and_can_clear_a_field() {
    let server = TestServer::start().await;
    server.register("kofi@example.test").await;
    let (account, row) = wallet_with_a_row(&server).await;
    let categories: Value = server.get("/categories").await.json().await.unwrap();
    let salary = categories
        .as_array()
        .unwrap()
        .iter()
        .find(|category| category["name"] == "Salary")
        .unwrap()["id"]
        .clone();

    let changed: Value = server
        .patch(
            &format!("/transactions/{row}"),
            json!({ "direction": "in", "amount": "12.50",
                    "booked_at": "2026-12-01T09:00:00Z", "label": "Pay",
                    "counterparty": null, "note": "Owed from October",
                    "status": "pending", "category_id": salary }),
        )
        .await
        .json()
        .await
        .unwrap();

    assert_eq!(changed["direction"], "in");
    assert!(changed["counterparty"].is_null());
    assert_eq!(changed["note"], "Owed from October");
    assert_eq!(changed["status"], "pending");
    assert_eq!(changed["category_id"], salary);
    assert_eq!(changed["booked_at"], "2026-12-01T09:00:00Z");
    // Now pending money in, so it is in the forecast and not in what is held.
    let accounts: Value = server.get("/accounts").await.json().await.unwrap();
    assert_eq!(accounts[0]["confirmed"]["display"], "100.00");
    assert_eq!(accounts[0]["projected"]["display"], "112.50");
    let _ = account;
}

#[tokio::test]
async fn an_edit_the_model_refuses_changes_nothing() {
    let server = TestServer::start().await;
    server.register("esi@example.test").await;
    let (account, row) = wallet_with_a_row(&server).await;

    for (body, field) in [
        (whole("12.505", "Waakye"), "amount"),
        (whole("12.50", "   "), "label"),
        (
            json!({ "direction": "sideways", "amount": "1", "booked_at": "2026-11-10T12:00:00Z",
                    "label": "x", "status": "confirmed" }),
            "direction",
        ),
        (
            json!({ "direction": "out", "amount": "1", "booked_at": "yesterday",
                    "label": "x", "status": "confirmed" }),
            "booked_at",
        ),
    ] {
        let response = server.patch(&format!("/transactions/{row}"), body).await;
        assert_eq!(response.status(), 422, "{field}");
        let refusal: Value = response.json().await.unwrap();
        assert!(
            refusal["message"].as_str().unwrap().contains(field),
            "{field}: {refusal}"
        );
    }
    assert_eq!(closing(&server, &account).await, "87.50");
}

#[tokio::test]
async fn a_deleted_row_leaves_the_ledger_and_undo_brings_it_back() {
    let server = TestServer::start().await;
    server.register("yaw@example.test").await;
    let (account, row) = wallet_with_a_row(&server).await;

    let response = server.delete(&format!("/transactions/{row}")).await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    assert_eq!(closing(&server, &account).await, "100.00");
    let rows: Value = server.get("/transactions").await.json().await.unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 0);

    // A deleted row cannot be changed or deleted again: it is not there.
    assert_eq!(
        server
            .patch(&format!("/transactions/{row}"), whole("1", "x"))
            .await
            .status(),
        404
    );
    assert_eq!(
        server
            .delete(&format!("/transactions/{row}"))
            .await
            .status(),
        404
    );

    let restored = server
        .post(&format!("/transactions/{row}/restore"), json!({}))
        .await;
    assert_eq!(restored.status(), 200, "{}", restored.text().await.unwrap());
    let restored: Value = restored.json().await.unwrap();
    assert_eq!(restored["label"], "Waakye");
    assert_eq!(restored["counterparty"], "Auntie Muni");
    assert_eq!(closing(&server, &account).await, "87.50");

    // And a live row has nothing to restore.
    assert_eq!(
        server
            .post(&format!("/transactions/{row}/restore"), json!({}))
            .await
            .status(),
        404
    );
}

#[tokio::test]
async fn changing_or_deleting_needs_the_csrf_token() {
    let server = TestServer::start().await;
    server.register("abena@example.test").await;
    let (_account, row) = wallet_with_a_row(&server).await;

    let response = server
        .client
        .delete(format!("{}/transactions/{row}", server.base))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn nobody_can_change_delete_or_restore_another_persons_row() {
    let ama = TestServer::start().await;
    ama.register("ama@example.test").await;
    let (account, row) = wallet_with_a_row(&ama).await;
    let kofi = ama.another_browser();
    kofi.register("kofi@example.test").await;

    assert_eq!(
        kofi.patch(&format!("/transactions/{row}"), whole("1", "Mine now"))
            .await
            .status(),
        404
    );
    assert_eq!(
        kofi.delete(&format!("/transactions/{row}")).await.status(),
        404
    );

    // Even once Ama has deleted it, Kofi cannot bring it back.
    ama.delete(&format!("/transactions/{row}")).await;
    let response = kofi
        .post(&format!("/transactions/{row}/restore"), json!({}))
        .await;
    assert_eq!(response.status(), 404);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "not_found");

    assert_eq!(closing(&ama, &account).await, "100.00");
}
