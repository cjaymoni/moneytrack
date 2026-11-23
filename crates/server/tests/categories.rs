//! Categories: what a ledger starts with, how the tree is kept one level deep, and filing a
//! row under one.

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

async fn wallet(server: &TestServer) -> String {
    let body: Value = server
        .post(
            "/accounts",
            json!({ "name": "Wallet", "kind": "momo", "currency": "GHS",
                    "opening_balance": "100" }),
        )
        .await
        .json()
        .await
        .unwrap();
    body["id"].as_str().unwrap().to_owned()
}

async fn categories(server: &TestServer) -> Vec<Value> {
    let body: Value = server.get("/categories").await.json().await.unwrap();
    body.as_array().unwrap().clone()
}

fn named<'a>(list: &'a [Value], name: &str) -> &'a Value {
    list.iter()
        .find(|category| category["name"] == name)
        .unwrap_or_else(|| panic!("no category called {name}"))
}

async fn add(server: &TestServer, body: Value) -> reqwest::Response {
    server.post("/categories", body).await
}

#[tokio::test]
async fn a_new_ledger_starts_with_categories_on_both_sides() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;

    let list = categories(&server).await;

    assert_eq!(list.len(), mt_server::repo::categories::STARTERS.len());
    assert_eq!(named(&list, "Transport")["kind"], "expense");
    assert_eq!(named(&list, "Salary")["kind"], "income");
    // In the order they were written, so the picker's first chips are the everyday ones.
    assert_eq!(list[0]["name"], "Food & groceries");
    assert!(list.iter().all(|category| category["parent_id"].is_null()));
}

#[tokio::test]
async fn a_category_can_sit_under_another_of_its_kind() {
    let server = TestServer::start().await;
    server.register("kofi@example.test").await;
    let transport = named(&categories(&server).await, "Transport")["id"].clone();

    let response = add(
        &server,
        json!({ "name": "Trotro", "kind": "expense", "parent_id": transport }),
    )
    .await;

    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let child: Value = response.json().await.unwrap();
    assert_eq!(child["parent_id"], transport);
    // Added at the end of the order, not in front of the starters.
    let list = categories(&server).await;
    assert_eq!(list.last().unwrap()["name"], "Trotro");
}

#[tokio::test]
async fn nesting_stops_at_one_level() {
    let server = TestServer::start().await;
    server.register("esi@example.test").await;
    let transport = named(&categories(&server).await, "Transport")["id"].clone();
    let trotro: Value = add(
        &server,
        json!({ "name": "Trotro", "kind": "expense", "parent_id": transport }),
    )
    .await
    .json()
    .await
    .unwrap();

    let response = add(
        &server,
        json!({ "name": "Madina route", "kind": "expense", "parent_id": trotro["id"] }),
    )
    .await;

    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "invalid");
}

#[tokio::test]
async fn a_category_cannot_sit_under_the_other_kind() {
    let server = TestServer::start().await;
    server.register("yaw@example.test").await;
    let salary = named(&categories(&server).await, "Salary")["id"].clone();

    let response = add(
        &server,
        json!({ "name": "Lunch", "kind": "expense", "parent_id": salary }),
    )
    .await;

    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["message"].as_str().unwrap().contains("parent_id"),
        "the refusal does not name the field: {body}"
    );
}

#[tokio::test]
async fn a_blank_name_is_refused() {
    let server = TestServer::start().await;
    server.register("abena@example.test").await;

    let response = add(&server, json!({ "name": "   ", "kind": "expense" })).await;

    assert_eq!(response.status(), 422);
}

#[tokio::test]
async fn a_row_is_recorded_under_a_category_and_can_be_refiled() {
    let server = TestServer::start().await;
    server.register("akos@example.test").await;
    let account = wallet(&server).await;
    let list = categories(&server).await;
    let transport = named(&list, "Transport")["id"].clone();
    let food = named(&list, "Food & groceries")["id"].clone();

    let written: Value = server
        .post(
            "/transactions",
            json!({ "account_id": account, "direction": "out", "amount": "5",
                    "label": "Kenkey", "category_id": transport }),
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(written["category_id"], transport);

    let id = written["id"].as_str().unwrap();
    let refiled = server
        .post(
            &format!("/transactions/{id}/category"),
            json!({ "category_id": food }),
        )
        .await;
    assert_eq!(refiled.status(), 200, "{}", refiled.text().await.unwrap());

    let ledger: Value = server
        .get(&format!("/ledger?account_id={account}"))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(ledger["entries"][0]["transaction"]["category_id"], food);
    // Refiling moves nothing: the balance is the same walk.
    assert_eq!(ledger["closing"]["display"], "95.00");

    let cleared: Value = server
        .post(
            &format!("/transactions/{id}/category"),
            json!({ "category_id": null }),
        )
        .await
        .json()
        .await
        .unwrap();
    assert!(cleared["category_id"].is_null());
}

#[tokio::test]
async fn a_category_that_does_not_exist_is_not_found() {
    let server = TestServer::start().await;
    server.register("kwame@example.test").await;
    let account = wallet(&server).await;

    let response = server
        .post(
            "/transactions",
            json!({ "account_id": account, "direction": "out", "amount": "5",
                    "label": "Kenkey",
                    "category_id": "0190e1c2-0000-7000-8000-00000000dead" }),
        )
        .await;

    assert_eq!(response.status(), 404);
    let rows: Value = server.get("/transactions").await.json().await.unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 0);
}
