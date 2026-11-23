//! Budgets over the wire: where a cap stands, what counts against it, and how a change to
//! it leaves the past alone.

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
use time::{Date, OffsetDateTime, Time};

/// The first instant of this month in UTC, plus a second: always in the past, always in
/// this month, whatever day the tests run on.
fn start_of_month() -> String {
    let today = OffsetDateTime::now_utc().date();
    let first = Date::from_calendar_date(today.year(), today.month(), 1).unwrap();
    first
        .with_time(Time::from_hms(0, 0, 1).unwrap())
        .assume_utc()
        .format(&Rfc3339)
        .unwrap()
}

async fn account(server: &TestServer, currency: &str) -> String {
    let body: Value = server
        .post(
            "/accounts",
            json!({ "name": currency, "kind": "momo", "currency": currency,
                    "opening_balance": "10000" }),
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

async fn spend(
    server: &TestServer,
    account: &str,
    amount: &str,
    category: Option<&str>,
    extra: Value,
) {
    let mut body = json!({ "account_id": account, "direction": "out", "amount": amount,
                           "label": "Row", "booked_at": start_of_month() });
    if let Some(category) = category {
        body["category_id"] = json!(category);
    }
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    let response = server.post("/transactions", body).await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
}

async fn set(server: &TestServer, body: Value) -> reqwest::Response {
    server.post("/budgets", body).await
}

async fn first(server: &TestServer) -> Value {
    let all: Value = server.get("/budgets").await.json().await.unwrap();
    all[0].clone()
}

#[tokio::test]
async fn a_cap_goes_from_under_to_warning_to_over_as_spending_climbs() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;
    let wallet = account(&server, "GHS").await;
    let food = category(&server, "Food & groceries").await;

    let response = set(
        &server,
        json!({ "category_id": food, "period": "monthly", "cap": "200", "currency": "GHS" }),
    )
    .await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let set_now: Value = response.json().await.unwrap();
    assert_eq!(set_now["name"], "Food & groceries");
    assert_eq!(set_now["warn_at_bps"], 8_000);
    assert_eq!(set_now["now"]["level"], "under");
    assert!(set_now["starts_on"].as_str().unwrap().ends_with("-01"));

    spend(&server, &wallet, "150", Some(&food), json!({})).await;
    assert_eq!(first(&server).await["now"]["level"], "under");

    spend(&server, &wallet, "20", Some(&food), json!({})).await;
    let warned = first(&server).await;
    assert_eq!(warned["now"]["level"], "warning");
    assert_eq!(warned["now"]["used_display"], "85%");
    assert_eq!(warned["now"]["remaining"]["display"], "30.00");

    spend(&server, &wallet, "40", Some(&food), json!({})).await;
    let over = first(&server).await;
    assert_eq!(over["now"]["level"], "over");
    assert_eq!(over["now"]["used_bps"], 10_500);
    assert_eq!(over["now"]["used_display"], "105%");
    assert_eq!(over["now"]["over_by"]["display"], "10.00");
}

#[tokio::test]
async fn children_count_other_currencies_do_not_and_expected_rows_are_shown_apart() {
    let server = TestServer::start().await;
    server.register("kofi@example.test").await;
    let cedis = account(&server, "GHS").await;
    let dollars = account(&server, "USD").await;
    let transport = category(&server, "Transport").await;
    let trotro: Value = server
        .post(
            "/categories",
            json!({ "name": "Trotro", "kind": "expense", "parent_id": transport }),
        )
        .await
        .json()
        .await
        .unwrap();
    let trotro = trotro["id"].as_str().unwrap().to_owned();

    set(
        &server,
        json!({ "category_id": transport, "period": "monthly", "cap": "100",
                "currency": "GHS" }),
    )
    .await;
    spend(&server, &cedis, "30", Some(&trotro), json!({})).await;
    spend(&server, &dollars, "50", Some(&transport), json!({})).await;
    spend(
        &server,
        &cedis,
        "60",
        Some(&transport),
        json!({ "status": "pending" }),
    )
    .await;

    let budget = first(&server).await;
    // The child's 30 counts; the dollars do not; the expected 60 counts only in `expected`.
    assert_eq!(budget["now"]["spent"]["display"], "30.00");
    assert_eq!(budget["now"]["level"], "under");
    assert_eq!(budget["expected"]["spent"]["display"], "90.00");
    assert_eq!(budget["expected"]["level"], "warning");
}

#[tokio::test]
async fn a_cap_on_all_spending_counts_every_category_and_none() {
    let server = TestServer::start().await;
    server.register("esi@example.test").await;
    let wallet = account(&server, "GHS").await;
    let food = category(&server, "Food & groceries").await;

    set(
        &server,
        json!({ "period": "monthly", "cap": "100", "currency": "GHS" }),
    )
    .await;
    spend(&server, &wallet, "40", Some(&food), json!({})).await;
    spend(&server, &wallet, "25", None, json!({})).await;

    let budget = first(&server).await;
    assert_eq!(budget["name"], "All spending");
    assert!(budget["category_id"].is_null());
    assert_eq!(budget["now"]["spent"]["display"], "65.00");
}

#[tokio::test]
async fn one_scope_has_one_cap_at_a_time() {
    let server = TestServer::start().await;
    server.register("yaw@example.test").await;
    let food = category(&server, "Food & groceries").await;
    let cap = |currency: &str| json!({ "category_id": food, "period": "monthly", "cap": "100", "currency": currency });

    assert_eq!(set(&server, cap("GHS")).await.status(), 200);
    let again = set(&server, cap("GHS")).await;
    assert_eq!(again.status(), 409);
    let body: Value = again.json().await.unwrap();
    assert_eq!(body["code"], "conflict");
    assert!(body["message"].as_str().unwrap().contains("already a monthly GHS cap"));

    // Another currency is another budget.
    assert_eq!(set(&server, cap("USD")).await.status(), 200);
}

#[tokio::test]
async fn a_new_budget_refuses_what_will_not_read() {
    let server = TestServer::start().await;
    server.register("abena@example.test").await;

    for (body, field) in [
        (
            json!({ "period": "monthly", "cap": "0", "currency": "GHS" }),
            "budget",
        ),
        (
            json!({ "period": "monthly", "cap": "1.005", "currency": "GHS" }),
            "cap",
        ),
        (
            json!({ "period": "daily", "cap": "10", "currency": "GHS" }),
            "period",
        ),
        (
            json!({ "period": "monthly", "cap": "10", "currency": "GHS", "warn_at_bps": 12000 }),
            "warn_at_bps",
        ),
    ] {
        let response = set(&server, body).await;
        assert_eq!(response.status(), 422, "{field}");
        let refusal: Value = response.json().await.unwrap();
        assert!(
            refusal["message"].as_str().unwrap().contains(field),
            "{field}: {refusal}"
        );
    }
}

#[tokio::test]
async fn changing_a_cap_in_its_first_period_changes_it_in_place() {
    let server = TestServer::start().await;
    server.register("akos@example.test").await;
    let set_now: Value = set(
        &server,
        json!({ "period": "monthly", "cap": "100", "currency": "GHS" }),
    )
    .await
    .json()
    .await
    .unwrap();
    let id = set_now["id"].as_str().unwrap();

    let changed: Value = server
        .patch(&format!("/budgets/{id}"), json!({ "cap": "150" }))
        .await
        .json()
        .await
        .unwrap();

    assert_eq!(changed["id"], id);
    assert_eq!(changed["cap"]["display"], "150.00");
    let all: Value = server.get("/budgets").await.json().await.unwrap();
    assert_eq!(all.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn changing_an_older_cap_leaves_past_periods_measured_against_the_old_one() {
    let server = TestServer::start().await;
    server.register("kwame@example.test").await;
    let set_now: Value = set(
        &server,
        json!({ "period": "monthly", "cap": "100", "currency": "GHS" }),
    )
    .await
    .json()
    .await
    .unwrap();
    let id = set_now["id"].as_str().unwrap();
    // As if it had been set in January: there is history to protect.
    server
        .run_sql(&format!(
            "UPDATE budgets SET starts_on = date_trunc('year', now())::date - 365 WHERE id = '{id}'"
        ))
        .await;

    let changed: Value = server
        .patch(
            &format!("/budgets/{id}"),
            json!({ "cap": "150", "warn_at_bps": 9000 }),
        )
        .await
        .json()
        .await
        .unwrap();

    assert_ne!(changed["id"], id, "a new budget carries the new cap");
    assert_eq!(changed["cap"]["display"], "150.00");
    assert_eq!(changed["warn_at_bps"], 9_000);
    assert!(changed["starts_on"].as_str().unwrap().ends_with("-01"));
    assert_eq!(changed["now"]["level"], "under");

    // The old one is ended, not removed: it no longer runs today, so it has no standing,
    // but last month is still measured against 100.
    let all: Value = server.get("/budgets").await.json().await.unwrap();
    let all = all.as_array().unwrap();
    assert_eq!(all.len(), 2);
    let old = all.iter().find(|budget| budget["id"] == id).unwrap();
    assert_eq!(old["cap"]["display"], "100.00");
    assert!(old["ends_on"].is_string());
    assert!(old["now"].is_null());
}

#[tokio::test]
async fn a_removed_budget_is_gone_and_another_person_cannot_touch_one() {
    let ama = TestServer::start().await;
    ama.register("ama@example.test").await;
    let set_now: Value = set(
        &ama,
        json!({ "period": "monthly", "cap": "100", "currency": "GHS" }),
    )
    .await
    .json()
    .await
    .unwrap();
    let id = set_now["id"].as_str().unwrap();
    let kofi = ama.another_browser();
    kofi.register("kofi@example.test").await;

    let theirs: Value = kofi.get("/budgets").await.json().await.unwrap();
    assert_eq!(theirs.as_array().unwrap().len(), 0);
    assert_eq!(
        kofi.patch(&format!("/budgets/{id}"), json!({ "cap": "1" }))
            .await
            .status(),
        404
    );
    assert_eq!(kofi.delete(&format!("/budgets/{id}")).await.status(), 404);
    let food = category(&ama, "Food & groceries").await;
    // Kofi cannot cap Ama's category either.
    assert_eq!(
        set(
            &kofi,
            json!({ "category_id": food, "period": "monthly", "cap": "5", "currency": "GHS" })
        )
        .await
        .status(),
        404
    );

    assert_eq!(ama.delete(&format!("/budgets/{id}")).await.status(), 200);
    let left: Value = ama.get("/budgets").await.json().await.unwrap();
    assert_eq!(left.as_array().unwrap().len(), 0);
    // And with it gone, the same scope can be capped again.
    assert_eq!(
        set(
            &ama,
            json!({ "period": "monthly", "cap": "100", "currency": "GHS" })
        )
        .await
        .status(),
        200
    );
}

#[tokio::test]
async fn the_default_warning_matches_the_schemas() {
    let server = TestServer::start().await;
    server.register("dede@example.test").await;
    set(
        &server,
        json!({ "period": "monthly", "cap": "100", "currency": "GHS" }),
    )
    .await;
    server
        .run_sql(
            "INSERT INTO budgets (user_id, id, period, cap_minor, currency, starts_on,
                                  created_at, updated_at, device_id)
             SELECT user_id, gen_random_uuid(), 'yearly', 100, 'GHS', now()::date, now(), now(),
                    device_id
               FROM budgets LIMIT 1",
        )
        .await;

    let all: Value = server.get("/budgets").await.json().await.unwrap();
    let defaults: Vec<u64> = all
        .as_array()
        .unwrap()
        .iter()
        .map(|budget| budget["warn_at_bps"].as_u64().unwrap())
        .collect();
    assert_eq!(
        defaults,
        [u64::from(mt_server::api::budgets::DEFAULT_WARN_BPS); 2]
    );
}
