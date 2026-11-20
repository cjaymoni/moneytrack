//! That one person cannot reach another's money.
//!
//! The most important file in this crate's tests. A finance API that leaks a row across
//! accounts has failed at the only thing it had to get right, and the failure is usually
//! silent: nothing errors, the wrong rows simply appear. So each of these asks for someone
//! else's data in a way that would work if the scoping were missing.

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

/// Two signed-in people on one server, in separate browsers.
async fn two_people() -> (TestServer, TestServer, String) {
    let ama = TestServer::start().await;
    ama.register("ama@example.test").await;
    let response = ama
        .post(
            "/accounts",
            json!({ "name": "Ama's wallet", "kind": "momo", "currency": "GHS",
                    "opening_balance_minor": 50_000 }),
        )
        .await;
    let account: Value = response.json().await.unwrap();
    let account = account["id"].as_str().unwrap().to_owned();

    ama.post(
        "/transactions",
        json!({ "account_id": account, "direction": "out", "amount_minor": 2_500,
                "label": "Ama's trotro" }),
    )
    .await;

    let kofi = ama.another_browser();
    kofi.register("kofi@example.test").await;
    (ama, kofi, account)
}

#[tokio::test]
async fn one_persons_accounts_are_not_anothers() {
    let (_ama, kofi, _account) = two_people().await;

    let accounts: Value = kofi.get("/accounts").await.json().await.unwrap();

    assert_eq!(
        accounts.as_array().unwrap().len(),
        0,
        "another person's accounts are visible: {accounts}"
    );
}

#[tokio::test]
async fn one_persons_transactions_are_not_anothers() {
    let (_ama, kofi, _account) = two_people().await;

    let rows: Value = kofi.get("/transactions").await.json().await.unwrap();

    assert_eq!(
        rows.as_array().unwrap().len(),
        0,
        "another person's transactions are visible: {rows}"
    );
}

#[tokio::test]
async fn naming_someone_elses_account_finds_nothing() {
    let (_ama, kofi, account) = two_people().await;

    let response = kofi
        .get(&format!("/transactions?account_id={account}"))
        .await;

    // Not found rather than forbidden: "forbidden" would confirm the account exists, which
    // is itself something Kofi is not entitled to learn.
    assert_eq!(response.status(), 404);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "not_found");
}

#[tokio::test]
async fn someone_elses_ledger_finds_nothing() {
    let (_ama, kofi, account) = two_people().await;

    let response = kofi.get(&format!("/ledger?account_id={account}")).await;

    assert_eq!(response.status(), 404);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "not_found");
}

#[tokio::test]
async fn writing_into_someone_elses_account_is_refused() {
    let (ama, kofi, account) = two_people().await;

    let response = kofi
        .post(
            "/transactions",
            json!({ "account_id": account, "direction": "out", "amount_minor": 100_000,
                    "label": "Not mine to spend" }),
        )
        .await;

    assert_eq!(response.status(), 404);

    // And Ama's balance is untouched: 500.00 opening, less her own 25.00 fare.
    let accounts: Value = ama.get("/accounts").await.json().await.unwrap();
    assert_eq!(accounts[0]["confirmed"]["minor_units"], 47_500);
}

#[tokio::test]
async fn each_persons_totals_are_their_own() {
    let (ama, kofi, _account) = two_people().await;

    let ama_totals: Value = ama.get("/totals").await.json().await.unwrap();
    let kofi_totals: Value = kofi.get("/totals").await.json().await.unwrap();

    assert_eq!(ama_totals[0]["minor_units"], 47_500);
    assert_eq!(
        kofi_totals.as_array().unwrap().len(),
        0,
        "a person with no accounts has a total: {kofi_totals}"
    );
}

#[tokio::test]
async fn signing_out_one_browser_leaves_the_other_signed_in() {
    let (ama, kofi, _account) = two_people().await;

    kofi.post("/auth/logout", json!({})).await;

    assert_eq!(kofi.get("/me").await.status(), 401);
    assert_eq!(ama.get("/me").await.status(), 200);
}

/// Every accessor in the repository layer takes a `UserId`.
///
/// Read off the source rather than exercised, because the thing being prevented is a
/// *future* query written without one. A test that called today's functions would pass for
/// ever while someone added the one that leaks.
#[test]
fn no_query_helper_exists_without_a_user() {
    // `users::create` and `users::by_email` are the two that cannot take one: they are how
    // a person is found before there is a user to scope to. Everything else, including
    // every ledger query, must.
    const BEFORE_THERE_IS_A_USER: [&str; 2] = ["pub async fn create", "pub async fn by_email"];

    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/repo");
    let mut checked = 0;

    for entry in std::fs::read_dir(&directory).expect("read the repo directory") {
        let path = entry.expect("read an entry").path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("read a repo module");
        let is_users = path.file_name().and_then(|name| name.to_str()) == Some("users.rs");

        for (index, _) in source.match_indices("pub async fn ") {
            let signature: String = source
                .get(index..)
                .unwrap_or_default()
                .chars()
                .take_while(|character| *character != '{')
                .collect();
            let exempt = is_users
                && BEFORE_THERE_IS_A_USER
                    .iter()
                    .any(|allowed| signature.starts_with(allowed));
            if exempt {
                continue;
            }
            assert!(
                signature.contains("UserId"),
                "{}: this accessor takes no UserId:\n{signature}",
                path.display()
            );
            checked += 1;
        }
    }

    assert!(checked >= 6, "only {checked} accessors were checked");
}
