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

use base64ct::Encoding;
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

// ------------------------------------------------------------- renaming and deleting ----

async fn spend(server: &TestServer, account: &str, label: &str, category: Option<&str>) -> String {
    let mut body = json!({ "account_id": account, "direction": "out", "amount": "5",
                           "label": label, "counterparty": label });
    if let Some(category) = category {
        body["category_id"] = json!(category);
    }
    let response = server.post("/transactions", body).await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let row: Value = response.json().await.unwrap();
    row["id"].as_str().unwrap().to_owned()
}

async fn rows(server: &TestServer, account: &str) -> Vec<Value> {
    let body: Value = server
        .get(&format!("/transactions?account_id={account}"))
        .await
        .json()
        .await
        .unwrap();
    body.as_array().unwrap().clone()
}

#[tokio::test]
async fn a_category_can_be_renamed() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;
    let list = categories(&server).await;
    let transport = named(&list, "Transport")["id"].as_str().unwrap().to_owned();

    let response = server
        .patch(
            &format!("/categories/{transport}"),
            json!({ "name": "  Trotro & taxi " }),
        )
        .await;

    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    assert_eq!(
        named(&categories(&server).await, "Trotro & taxi")["id"],
        transport.as_str()
    );
    let blank = server
        .patch(&format!("/categories/{transport}"), json!({ "name": "  " }))
        .await;
    assert_eq!(blank.status(), 422);
}

#[tokio::test]
async fn deleting_a_category_moves_its_transactions_where_the_person_says() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;
    let account = wallet(&server).await;
    let list = categories(&server).await;
    let eating_out = named(&list, "Eating out")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let food = named(&list, "Food & groceries")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    spend(&server, &account, "Waakye", Some(&eating_out)).await;

    let response = server
        .delete(&format!("/categories/{eating_out}?move_to={food}"))
        .await;

    assert_eq!(response.status(), 204, "{}", response.text().await.unwrap());
    assert!(
        categories(&server)
            .await
            .iter()
            .all(|each| each["id"] != eating_out.as_str())
    );
    assert_eq!(
        rows(&server, &account).await[0]["category_id"],
        food.as_str()
    );
}

#[tokio::test]
async fn deleting_a_category_with_nowhere_to_go_leaves_its_transactions_uncategorised() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;
    let account = wallet(&server).await;
    let clothing = named(&categories(&server).await, "Clothing")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    spend(&server, &account, "Kaba", Some(&clothing)).await;

    assert_eq!(
        server
            .delete(&format!("/categories/{clothing}"))
            .await
            .status(),
        204
    );
    assert!(rows(&server, &account).await[0]["category_id"].is_null());
}

#[tokio::test]
async fn a_category_with_subcategories_a_budget_or_the_wrong_target_is_kept() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;
    let list = categories(&server).await;
    let transport = named(&list, "Transport")["id"].as_str().unwrap().to_owned();
    let health = named(&list, "Health")["id"].as_str().unwrap().to_owned();
    let salary = named(&list, "Salary")["id"].as_str().unwrap().to_owned();
    add(
        &server,
        json!({ "name": "Trotro", "kind": "expense", "parent_id": transport }),
    )
    .await;
    let budget = server
        .post(
            "/budgets",
            json!({ "category_id": health, "period": "monthly", "cap": "100", "currency": "GHS" }),
        )
        .await;
    assert_eq!(budget.status(), 200, "{}", budget.text().await.unwrap());

    let with_child = server.delete(&format!("/categories/{transport}")).await;
    assert_eq!(with_child.status(), 409);
    let with_budget = server.delete(&format!("/categories/{health}")).await;
    assert_eq!(with_budget.status(), 409);
    let giving = named(&list, "Giving")["id"].as_str().unwrap().to_owned();
    let wrong_kind = server
        .delete(&format!("/categories/{giving}?move_to={salary}"))
        .await;
    assert_eq!(wrong_kind.status(), 422);
    let into_itself = server
        .delete(&format!("/categories/{salary}?move_to={salary}"))
        .await;
    assert_eq!(into_itself.status(), 422);
    assert_eq!(categories(&server).await.len(), list.len() + 1);
}

#[tokio::test]
async fn someone_elses_category_cannot_be_renamed_or_deleted() {
    let ama = TestServer::start().await;
    ama.register("ama@example.test").await;
    let hers = named(&categories(&ama).await, "Transport")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let kofi = ama.another_browser();
    kofi.register("kofi@example.test").await;

    let renamed = kofi
        .patch(
            &format!("/categories/{hers}"),
            json!({ "name": "Mine now" }),
        )
        .await;
    assert_eq!(renamed.status(), 404);
    assert_eq!(
        kofi.delete(&format!("/categories/{hers}")).await.status(),
        404
    );
    assert_eq!(
        named(&categories(&ama).await, "Transport")["id"],
        hers.as_str()
    );
}

// ------------------------------------------------------------------ filing at once ----

#[tokio::test]
async fn several_rows_are_filed_at_once_and_the_payee_is_remembered() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;
    let account = wallet(&server).await;
    let eating_out = named(&categories(&server).await, "Eating out")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let first = spend(&server, &account, "KFC OSU", None).await;
    let second = spend(&server, &account, "KFC OSU", None).await;

    let response = server
        .post(
            "/transactions/category",
            json!({ "transaction_ids": [first, second], "category_id": eating_out,
                    "remember": { "matched_field": "counterparty", "pattern": "KFC OSU" } }),
        )
        .await;

    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["filed"], 2);
    assert!(
        rows(&server, &account)
            .await
            .iter()
            .all(|row| row["category_id"] == eating_out.as_str())
    );

    // The next statement from that payee is proposed under the same category.
    let mapping = json!({ "delimiter": "comma", "skip_rows": 0, "has_header": true, "date": 0,
                          "date_format": "day_month_year", "amount": { "kind": "signed", "column": 2 },
                          "label": 1, "counterparty": 3, "reference": null, "balance": null });
    let csv = "Date,Description,Amount,Payee\n09/10/2026,CARD 7,-30.00,KFC OSU\n";
    let upload = server
        .post(
            "/imports",
            json!({ "account_id": account, "file_name": "oct.csv", "format": "csv",
                    "content_base64": base64ct::Base64::encode_string(csv.as_bytes()),
                    "mapping": mapping, "offset_minutes": 0 }),
        )
        .await;
    assert_eq!(upload.status(), 200, "{}", upload.text().await.unwrap());
    let waiting: Value = server.get("/inbox").await.json().await.unwrap();
    assert_eq!(waiting["items"][0]["category_id"], eating_out.as_str());
}

#[tokio::test]
async fn filing_at_once_is_all_or_nothing() {
    let ama = TestServer::start().await;
    ama.register("ama@example.test").await;
    let account = wallet(&ama).await;
    let health = named(&categories(&ama).await, "Health")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mine = spend(&ama, &account, "Pharmacy", None).await;

    let kofi = ama.another_browser();
    kofi.register("kofi@example.test").await;
    let his_account = wallet(&kofi).await;
    let his = spend(&kofi, &his_account, "Chemist", None).await;

    let mixed = ama
        .post(
            "/transactions/category",
            json!({ "transaction_ids": [mine, his], "category_id": health }),
        )
        .await;
    assert_eq!(mixed.status(), 404);
    assert!(rows(&ama, &account).await[0]["category_id"].is_null());

    for (body, field) in [
        (
            json!({ "transaction_ids": [], "category_id": health }),
            "transaction_ids",
        ),
        (
            json!({ "transaction_ids": [mine, mine], "category_id": health }),
            "transaction_ids",
        ),
        (
            json!({ "transaction_ids": [mine], "category_id": null,
                    "remember": { "matched_field": "counterparty", "pattern": "Pharmacy" } }),
            "remember",
        ),
        (
            json!({ "transaction_ids": [mine], "category_id": health,
                    "remember": { "matched_field": "raw_text", "pattern": "Pharmacy" } }),
            "remember.matched_field",
        ),
    ] {
        let response = ama.post("/transactions/category", body).await;
        assert_eq!(response.status(), 422, "{field}");
        let message: Value = response.json().await.unwrap();
        assert!(
            message["message"].as_str().unwrap().contains(field),
            "{message}"
        );
    }
}
