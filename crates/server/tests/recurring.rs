//! Repeating rows: a series writes real expected rows a year ahead, keeps them topped up,
//! falls on the right days, and takes its future with it when it stops.

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
use time::{Date, Duration, Month, OffsetDateTime, Weekday};

fn today() -> Date {
    OffsetDateTime::now_utc().date()
}

fn spell(date: Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

async fn wallet(server: &TestServer) -> String {
    let body: Value = server
        .post(
            "/accounts",
            json!({ "name": "Wallet", "kind": "momo", "currency": "GHS",
                    "opening_balance": "1000" }),
        )
        .await
        .json()
        .await
        .unwrap();
    body["id"].as_str().unwrap().to_owned()
}

async fn start(server: &TestServer, body: Value) -> reqwest::Response {
    server.post("/recurring", body).await
}

/// The rows a series has written, as (local date, status).
async fn rows_of(server: &TestServer, series: &str) -> Vec<(String, String)> {
    let rows: Value = server.get("/transactions").await.json().await.unwrap();
    rows.as_array()
        .unwrap()
        .iter()
        .filter(|row| row["recurring_id"] == series)
        .map(|row| {
            (
                row["booked_at"].as_str().unwrap()[..10].to_owned(),
                row["status"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_monthly_series_writes_a_year_of_expected_rows_the_outlook_can_see() {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;
    let account = wallet(&server).await;
    let first = today() + Duration::days(5);

    let response = start(
        &server,
        json!({ "account_id": account, "direction": "out", "amount": "1400",
                "label": "Rent, Spintex", "freq": "monthly", "starts_on": spell(first) }),
    )
    .await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let series: Value = response.json().await.unwrap();
    let id = series["id"].as_str().unwrap();
    assert_eq!(series["next_on"], spell(first));
    assert_eq!(series["account_name"], "Wallet");

    let written = rows_of(&server, id).await;
    // Monthly, a year ahead: twelve or thirteen, depending on where in the month today is.
    assert!((12..=13).contains(&written.len()), "{written:?}");
    assert_eq!(written[0].0, spell(first));
    assert!(written.iter().all(|(_, status)| status == "pending"));

    let outlook: Value = server.get("/outlook?days=30").await.json().await.unwrap();
    let ahead = outlook[0]["points"].as_array().unwrap();
    assert_eq!(ahead[0]["transaction"]["label"], "Rent, Spintex");
    assert_eq!(ahead[0]["transaction"]["source"], "recurring");
}

#[tokio::test]
async fn reading_again_writes_nothing_twice() {
    let server = TestServer::start().await;
    server.register("kofi@example.test").await;
    let account = wallet(&server).await;
    let series: Value = start(
        &server,
        json!({ "account_id": account, "direction": "out", "amount": "5", "label": "Trotro",
                "freq": "weekly", "starts_on": spell(today() + Duration::days(1)) }),
    )
    .await
    .json()
    .await
    .unwrap();
    let id = series["id"].as_str().unwrap();
    let once = rows_of(&server, id).await.len();

    for _ in 0..3 {
        server.get(&format!("/ledger?account_id={account}")).await;
        server.get("/outlook").await;
    }

    assert_eq!(rows_of(&server, id).await.len(), once);
}

#[tokio::test]
async fn a_series_that_has_fallen_behind_is_topped_up_on_the_next_read() {
    let server = TestServer::start().await;
    server.register("esi@example.test").await;
    let account = wallet(&server).await;
    let series: Value = start(
        &server,
        json!({ "account_id": account, "direction": "in", "amount": "4200", "label": "Salary",
                "freq": "monthly", "starts_on": spell(today() + Duration::days(2)) }),
    )
    .await
    .json()
    .await
    .unwrap();
    let id = series["id"].as_str().unwrap();
    let full = rows_of(&server, id).await.len();

    // As if the year had been written long ago and time had moved on: only the next month
    // is left on the books.
    server
        .run_sql(&format!(
            "DELETE FROM transactions WHERE recurring_id = '{id}'
                AND booked_at > now() + interval '31 days';
             UPDATE recurring SET generated_through = current_date + 31 WHERE id = '{id}';"
        ))
        .await;
    // No read in between: every read tops up first, which is the behaviour under test.

    server.get(&format!("/ledger?account_id={account}")).await;

    assert_eq!(rows_of(&server, id).await.len(), full);
}

#[tokio::test]
async fn a_series_on_the_31st_falls_on_the_last_day_of_a_short_month_and_comes_back() {
    let server = TestServer::start().await;
    server.register("yaw@example.test").await;
    let account = wallet(&server).await;
    // The next 31st, at least a day away.
    let mut first = today() + Duration::days(1);
    while first.day() != 31 {
        first = first.next_day().unwrap();
    }
    let series: Value = start(
        &server,
        json!({ "account_id": account, "direction": "out", "amount": "100", "label": "Levy",
                "freq": "monthly", "starts_on": spell(first) }),
    )
    .await
    .json()
    .await
    .unwrap();

    let format = time::macros::format_description!("[year]-[month]-[day]");
    let dates: Vec<Date> = rows_of(&server, series["id"].as_str().unwrap())
        .await
        .iter()
        .map(|(date, _)| Date::parse(date, &format).unwrap())
        .collect();

    // Every row on the 31st, or on the last day of a month too short to have one.
    for date in &dates {
        let last = time::util::days_in_month(date.month(), date.year());
        assert_eq!(date.day(), last.min(31), "{date}");
    }
    let days: Vec<(Month, u8)> = dates
        .iter()
        .map(|date| (date.month(), date.day()))
        .collect();
    // And a short month did not stick: a 31-day month after a 30-day one is back on the 31st.
    assert!(
        days.iter().filter(|(_, day)| *day == 31).count() >= 6,
        "{days:?}"
    );
}

#[tokio::test]
async fn a_weekly_series_falls_on_its_starting_weekday() {
    let server = TestServer::start().await;
    server.register("abena@example.test").await;
    let account = wallet(&server).await;
    let first = today() + Duration::days(3);
    let series: Value = start(
        &server,
        json!({ "account_id": account, "direction": "out", "amount": "20", "label": "Market",
                "freq": "weekly", "interval": 2, "starts_on": spell(first) }),
    )
    .await
    .json()
    .await
    .unwrap();

    let written = rows_of(&server, series["id"].as_str().unwrap()).await;
    let format = time::macros::format_description!("[year]-[month]-[day]");
    let weekdays: Vec<Weekday> = written
        .iter()
        .map(|(date, _)| Date::parse(date, &format).unwrap().weekday())
        .collect();
    assert!(weekdays.iter().all(|day| *day == first.weekday()));
    // Every other week, a year ahead.
    assert!((26..=27).contains(&written.len()), "{}", written.len());
}

#[tokio::test]
async fn stopping_a_series_takes_its_future_and_leaves_its_past() {
    let server = TestServer::start().await;
    server.register("akos@example.test").await;
    let account = wallet(&server).await;
    // Started three days ago: three overdue expected rows, one today, and a year ahead.
    let series: Value = start(
        &server,
        json!({ "account_id": account, "direction": "out", "amount": "2", "label": "Water",
                "freq": "daily", "starts_on": spell(today() - Duration::days(3)) }),
    )
    .await
    .json()
    .await
    .unwrap();
    let id = series["id"].as_str().unwrap();
    let before = rows_of(&server, id).await;
    assert!(before.len() > 300);

    let response = server.delete(&format!("/recurring/{id}")).await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());

    let after = rows_of(&server, id).await;
    // Today's row was booked at midnight, which has passed: it stays with the past.
    assert_eq!(after.len(), 4, "{after:?}");
    assert!(
        after
            .iter()
            .all(|(date, _)| date.as_str() <= spell(today()).as_str())
    );
    let left: Value = server.get("/recurring").await.json().await.unwrap();
    assert_eq!(left.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn moving_one_occurrence_onto_anothers_day_is_refused_in_words() {
    let server = TestServer::start().await;
    server.register("dede@example.test").await;
    let account = wallet(&server).await;
    let series: Value = start(
        &server,
        json!({ "account_id": account, "direction": "out", "amount": "3", "label": "Bread",
                "freq": "daily", "starts_on": spell(today() + Duration::days(1)) }),
    )
    .await
    .json()
    .await
    .unwrap();
    let id = series["id"].as_str().unwrap();
    let rows: Value = server.get("/transactions").await.json().await.unwrap();
    let mut mine: Vec<&Value> = rows
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["recurring_id"] == id)
        .collect();
    mine.sort_by_key(|row| row["booked_at"].as_str().unwrap().to_owned());
    let (first, second) = (mine[0], mine[1]);

    let response = server
        .patch(
            &format!("/transactions/{}", second["id"].as_str().unwrap()),
            json!({ "direction": "out", "amount": "3", "booked_at": first["booked_at"],
                    "label": "Bread", "status": "pending" }),
        )
        .await;

    assert_eq!(response.status(), 409);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("same repeating series"),
        "{body}"
    );
}

#[tokio::test]
async fn a_series_that_will_not_read_names_the_field() {
    let server = TestServer::start().await;
    server.register("nii@example.test").await;
    let account = wallet(&server).await;
    let base = json!({ "account_id": account, "direction": "out", "amount": "10",
                       "label": "x", "freq": "monthly", "starts_on": spell(today()) });

    for (change, field) in [
        (
            json!({ "starts_on": spell(today() - Duration::days(400)) }),
            "starts_on",
        ),
        (json!({ "starts_on": "next tuesday" }), "starts_on"),
        (json!({ "interval": 0 }), "interval"),
        (json!({ "freq": "fortnightly" }), "freq"),
        (json!({ "amount": "0" }), "amount"),
        (json!({ "amount": "1.005" }), "amount"),
    ] {
        let mut body = base.clone();
        for (key, value) in change.as_object().unwrap() {
            body[key] = value.clone();
        }
        let response = start(&server, body).await;
        assert_eq!(response.status(), 422, "{field}");
        let refusal: Value = response.json().await.unwrap();
        assert!(
            refusal["message"].as_str().unwrap().contains(field),
            "{field}: {refusal}"
        );
    }
}

#[tokio::test]
async fn nobody_reaches_anothers_series() {
    let ama = TestServer::start().await;
    ama.register("ama@example.test").await;
    let account = wallet(&ama).await;
    let series: Value = start(
        &ama,
        json!({ "account_id": account, "direction": "out", "amount": "1400", "label": "Rent",
                "freq": "monthly", "starts_on": spell(today() + Duration::days(1)) }),
    )
    .await
    .json()
    .await
    .unwrap();
    let id = series["id"].as_str().unwrap();
    let kofi = ama.another_browser();
    kofi.register("kofi@example.test").await;

    let theirs: Value = kofi.get("/recurring").await.json().await.unwrap();
    assert_eq!(theirs.as_array().unwrap().len(), 0);
    assert_eq!(kofi.delete(&format!("/recurring/{id}")).await.status(), 404);
    // Nor start one in Ama's account.
    let response = start(
        &kofi,
        json!({ "account_id": account, "direction": "out", "amount": "1", "label": "Mine",
                "freq": "daily", "starts_on": spell(today()) }),
    )
    .await;
    assert_eq!(response.status(), 404);
    assert!(rows_of(&ama, id).await.len() >= 12);
}
