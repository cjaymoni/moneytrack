//! Importing a statement and reviewing it: staging, re-uploads, accepting, learning,
//! duplicates, merging, rejecting — and who may do any of it.

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use base64ct::{Base64, Encoding};
use common::TestServer;
use serde_json::{Value, json};

/// Date, description, signed amount, counterparty — the columns these tests' statements use.
fn mapping() -> Value {
    json!({
        "delimiter": "comma",
        "skip_rows": 0,
        "has_header": true,
        "date": 0,
        "date_format": "day_month_year",
        "amount": { "kind": "signed", "column": 2 },
        "label": 1,
        "counterparty": 3,
        "reference": null,
        "balance": null
    })
}

const STATEMENT: &str = "Date,Description,Amount,Payee\n\
03/10/2026,POS PURCHASE,-120.50,SHOPRITE\n\
04/10/2026,SALARY OCT,5000.00,ACME LTD\n\
05/10/2026,Closing balance,,\n";

async fn signed_in() -> TestServer {
    let server = TestServer::start().await;
    server.register("ama@example.test").await;
    server
}

async fn account(server: &TestServer, currency: &str) -> String {
    let body: Value = server
        .post(
            "/accounts",
            json!({ "name": "Bank", "kind": "bank", "currency": currency }),
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
        .unwrap_or_else(|| panic!("no category called {name}"))["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn upload(server: &TestServer, account: &str, format: &str, text: &str) -> reqwest::Response {
    server
        .post(
            "/imports",
            json!({
                "account_id": account,
                "file_name": "nov.csv",
                "format": format,
                "content_base64": Base64::encode_string(text.as_bytes()),
                "mapping": mapping(),
                "offset_minutes": 0
            }),
        )
        .await
}

async fn import(server: &TestServer, account: &str, text: &str) -> Value {
    let response = upload(server, account, "csv", text).await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    response.json().await.unwrap()
}

async fn inbox(server: &TestServer) -> Value {
    let response = server.get("/inbox").await;
    assert_eq!(response.status(), 200);
    response.json().await.unwrap()
}

fn item<'a>(inbox: &'a Value, label: &str) -> &'a Value {
    inbox["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["label"] == label)
        .unwrap_or_else(|| panic!("no waiting line labelled {label}: {inbox}"))
}

async fn accept(server: &TestServer, id: &str, category: Option<&str>) -> reqwest::Response {
    server
        .post(
            "/inbox/accept",
            json!({ "items": [{ "id": id, "category_id": category }] }),
        )
        .await
}

async fn ledger_rows(server: &TestServer, account: &str) -> Vec<Value> {
    let rows: Value = server
        .get(&format!("/transactions?account_id={account}"))
        .await
        .json()
        .await
        .unwrap();
    rows.as_array().unwrap().clone()
}

// --------------------------------------------------------------------------- uploading ----

#[tokio::test]
async fn a_preview_splits_the_file_and_later_offers_the_mapping_used_last() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    let preview = || {
        server.post(
            "/imports/preview",
            json!({ "account_id": bank,
                    "content_base64": Base64::encode_string(STATEMENT.as_bytes()) }),
        )
    };

    let first: Value = preview().await.json().await.unwrap();
    assert_eq!(first["delimiter"], "comma");
    assert_eq!(first["rows"][1][3], "SHOPRITE");
    assert_eq!(first["saved_mapping"], Value::Null);

    import(&server, &bank, STATEMENT).await;
    let later: Value = preview().await.json().await.unwrap();
    assert_eq!(later["saved_mapping"], mapping());
}

#[tokio::test]
async fn an_upload_stages_what_it_read_and_reports_what_it_could_not() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;

    let outcome = import(&server, &bank, STATEMENT).await;

    assert_eq!(outcome["read"], 2);
    assert_eq!(outcome["added"], 2);
    assert_eq!(outcome["problem_count"], 1);
    assert_eq!(outcome["problems"][0]["line_no"], 4);
    let waiting = inbox(&server).await;
    assert_eq!(waiting["total"], 2);
    let shop = item(&waiting, "POS PURCHASE");
    assert_eq!(shop["direction"], "out");
    assert_eq!(shop["amount"]["minor_units"], 12_050);
    assert_eq!(shop["file_name"], "nov.csv");
    assert_eq!(shop["line_no"], 2);
    assert!(
        ledger_rows(&server, &bank).await.is_empty(),
        "nothing reaches the ledger until it is accepted"
    );
}

#[tokio::test]
async fn uploading_the_same_statement_again_offers_nothing_twice() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    import(&server, &bank, STATEMENT).await;

    let again = import(&server, &bank, STATEMENT).await;

    assert_eq!(again["added"], 0);
    assert_eq!(again["already_seen"], 2);
    assert_eq!(inbox(&server).await["total"], 2);
}

#[tokio::test]
async fn a_statement_bigger_than_an_ordinary_request_still_uploads() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    let mut text = String::from("Date,Description,Amount,Payee\n");
    for day in 0..3_000 {
        text.push_str(&format!(
            "{:02}/10/2026,Fare {day},-1.00,TROTRO\n",
            day % 28 + 1
        ));
    }
    assert!(text.len() > 64 * 1024);

    let outcome = import(&server, &bank, &text).await;

    assert_eq!(outcome["added"], 3_000);
}

#[tokio::test]
async fn only_the_upload_routes_take_a_large_body() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;

    let oversized = server
        .post(
            "/transactions",
            json!({ "account_id": bank, "direction": "out", "amount": "1.00",
                    "label": "x".repeat(100 * 1024) }),
        )
        .await;

    assert_eq!(oversized.status(), 413);
}

#[tokio::test]
async fn an_upload_that_cannot_be_read_is_refused_and_stages_nothing() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;

    let no_mapping = server
        .post(
            "/imports",
            json!({ "account_id": bank, "format": "csv",
                    "content_base64": Base64::encode_string(STATEMENT.as_bytes()) }),
        )
        .await;
    assert_eq!(no_mapping.status(), 422);

    let not_base64 = server
        .post(
            "/imports",
            json!({ "account_id": bank, "format": "ofx", "content_base64": "%%%" }),
        )
        .await;
    assert_eq!(not_base64.status(), 422);

    let sms = upload(&server, &bank, "sms", STATEMENT).await;
    assert_eq!(sms.status(), 422);

    let ofx_in_dollars = "<OFX><STMTRS><CURDEF>USD<BANKTRANLIST>\
        <STMTTRN><DTPOSTED>20261003<TRNAMT>-1.00<NAME>X</BANKTRANLIST></STMTRS></OFX>";
    let wrong_currency = upload(&server, &bank, "ofx", ofx_in_dollars).await;
    assert_eq!(wrong_currency.status(), 422);

    assert_eq!(inbox(&server).await["total"], 0);
}

// --------------------------------------------------------------------------- accepting ----

#[tokio::test]
async fn accepting_a_line_writes_it_to_the_ledger_as_imported() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    import(&server, &bank, STATEMENT).await;
    let groceries = category(&server, "Food & groceries").await;
    let waiting = inbox(&server).await;
    let shop = item(&waiting, "POS PURCHASE")["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let response = accept(&server, &shop, Some(&groceries)).await;

    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let rows = ledger_rows(&server, &bank).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["source"], "csv");
    assert_eq!(rows[0]["status"], "confirmed");
    assert_eq!(rows[0]["category_id"], groceries.as_str());
    assert_eq!(rows[0]["counterparty"], "SHOPRITE");
    assert_eq!(inbox(&server).await["total"], 1);
}

#[tokio::test]
async fn a_line_cannot_be_settled_twice() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    import(&server, &bank, STATEMENT).await;
    let waiting = inbox(&server).await;
    let shop = item(&waiting, "POS PURCHASE")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(accept(&server, &shop, None).await.status(), 200);

    assert_eq!(accept(&server, &shop, None).await.status(), 409);
    let reject = server
        .post(&format!("/inbox/{shop}/reject"), json!({}))
        .await;
    assert_eq!(reject.status(), 409);
    assert_eq!(ledger_rows(&server, &bank).await.len(), 1);
}

#[tokio::test]
async fn a_batch_with_one_bad_line_accepts_none_of_them() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    import(&server, &bank, STATEMENT).await;
    let waiting = inbox(&server).await;
    let shop = item(&waiting, "POS PURCHASE")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let salary = item(&waiting, "SALARY OCT")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    server
        .post(&format!("/inbox/{salary}/reject"), json!({}))
        .await;

    let response = server
        .post(
            "/inbox/accept",
            json!({ "items": [{ "id": shop }, { "id": salary }] }),
        )
        .await;

    assert_eq!(response.status(), 409);
    assert!(ledger_rows(&server, &bank).await.is_empty());
    assert_eq!(inbox(&server).await["total"], 1);
}

#[tokio::test]
async fn filing_a_line_somewhere_new_files_that_merchant_there_next_time() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    import(&server, &bank, STATEMENT).await;
    let eating_out = category(&server, "Eating out").await;
    let waiting = inbox(&server).await;
    let shop = item(&waiting, "POS PURCHASE");
    assert_eq!(shop["category_id"], Value::Null);
    accept(&server, shop["id"].as_str().unwrap(), Some(&eating_out)).await;

    import(
        &server,
        &bank,
        "Date,Description,Amount,Payee\n09/10/2026,CARD 0412,-30.00,Shoprite Osu\n",
    )
    .await;

    let next = inbox(&server).await;
    let card = item(&next, "CARD 0412");
    assert_eq!(card["category_id"], eating_out.as_str());
    assert!(card["category_confidence_bps"].as_u64().unwrap() > 0);
}

// -------------------------------------------------------------------------- duplicates ----

#[tokio::test]
async fn a_line_already_typed_by_hand_is_offered_as_a_duplicate_and_can_be_merged() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    let typed: Value = server
        .post(
            "/transactions",
            json!({ "account_id": bank, "direction": "out", "amount": "120.50",
                    "label": "Shopping", "counterparty": "Shoprite",
                    "booked_at": "2026-10-03T14:00:00Z" }),
        )
        .await
        .json()
        .await
        .unwrap();
    import(&server, &bank, STATEMENT).await;

    let waiting = inbox(&server).await;
    let shop = item(&waiting, "POS PURCHASE");
    let suspects = shop["suspects"].as_array().unwrap();
    assert_eq!(suspects.len(), 1, "{shop}");
    assert_eq!(suspects[0]["transaction"]["id"], typed["id"]);
    assert_eq!(suspects[0]["certain"], false);
    assert!(
        item(&waiting, "SALARY OCT")["suspects"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let merged = server
        .post(
            &format!("/inbox/{}/merge", shop["id"].as_str().unwrap()),
            json!({ "transaction_id": typed["id"] }),
        )
        .await;
    assert_eq!(merged.status(), 200);
    assert_eq!(ledger_rows(&server, &bank).await.len(), 1);
    assert_eq!(inbox(&server).await["total"], 1);
}

#[tokio::test]
async fn merging_into_an_expected_row_confirms_it() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    let expected: Value = server
        .post(
            "/transactions",
            json!({ "account_id": bank, "direction": "in", "amount": "5000.00",
                    "label": "Salary", "counterparty": "ACME LTD", "status": "pending",
                    "booked_at": "2026-10-04T08:00:00Z" }),
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(expected["status"], "pending");
    import(&server, &bank, STATEMENT).await;
    let waiting = inbox(&server).await;
    let salary = item(&waiting, "SALARY OCT");

    server
        .post(
            &format!("/inbox/{}/merge", salary["id"].as_str().unwrap()),
            json!({ "transaction_id": expected["id"] }),
        )
        .await;

    let rows = ledger_rows(&server, &bank).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["status"], "confirmed");
}

#[tokio::test]
async fn two_identical_lines_of_one_statement_are_not_each_others_duplicate() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    import(
        &server,
        &bank,
        "Date,Description,Amount,Payee\n\
         03/10/2026,Trotro,-5.00,STATION\n\
         03/10/2026,Trotro,-5.00,STATION\n",
    )
    .await;
    let waiting = inbox(&server).await;
    assert_eq!(waiting["total"], 2);
    let first = waiting["items"][0]["id"].as_str().unwrap().to_owned();
    accept(&server, &first, None).await;

    let after = inbox(&server).await;

    assert!(
        after["items"][0]["suspects"].as_array().unwrap().is_empty(),
        "{after}"
    );
}

#[tokio::test]
async fn the_banks_own_reference_is_a_certain_match_and_cannot_be_accepted_twice() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    let ofx = |name: &str| {
        format!(
            "<OFX><STMTRS><CURDEF>GHS<BANKTRANLIST>\
             <STMTTRN><DTPOSTED>20261003<TRNAMT>-20.00<FITID>REF-1<NAME>{name}\
             </BANKTRANLIST></STMTRS></OFX>"
        )
    };
    assert_eq!(
        upload(&server, &bank, "ofx", &ofx("SHOPRITE"))
            .await
            .status(),
        200
    );
    let first = inbox(&server).await["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    accept(&server, &first, None).await;

    // The same movement, described differently by a later export.
    assert_eq!(
        upload(&server, &bank, "ofx", &ofx("SHOPRITE ACCRA"))
            .await
            .status(),
        200
    );
    let waiting = inbox(&server).await;
    let again = &waiting["items"][0];
    assert_eq!(again["suspects"][0]["certain"], true);

    let response = accept(&server, again["id"].as_str().unwrap(), None).await;
    assert_eq!(response.status(), 409);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["message"].as_str().unwrap().contains("same payment"),
        "{body}"
    );
    assert_eq!(ledger_rows(&server, &bank).await.len(), 1);
}

/// One OFX statement line with a bank reference.
fn ofx_line(fitid: &str, amount: &str, name: &str) -> String {
    format!(
        "<OFX><STMTRS><CURDEF>GHS<BANKTRANLIST>\
         <STMTTRN><DTPOSTED>20261003<TRNAMT>{amount}<FITID>{fitid}<NAME>{name}\
         </BANKTRANLIST></STMTRS></OFX>"
    )
}

#[tokio::test]
async fn a_charge_sharing_its_transfers_reference_is_kept_as_its_own_row_never_merged_away() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    let transfer = ofx_line("TRF-9", "-500.00", "TRANSFER TO KOFI");
    let levy = ofx_line("TRF-9", "-1.00", "TRANSFER TO KOFI");
    assert_eq!(upload(&server, &bank, "ofx", &transfer).await.status(), 200);
    let first = inbox(&server).await["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(accept(&server, &first, None).await.status(), 200);
    let sent = ledger_rows(&server, &bank).await[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // A later statement carries the charge, under the transfer's reference.
    assert_eq!(upload(&server, &bank, "ofx", &levy).await.status(), 200);
    let charge = inbox(&server).await["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let merged = server
        .post(
            &format!("/inbox/{charge}/merge"),
            json!({ "transaction_id": sent }),
        )
        .await;
    assert_eq!(merged.status(), 409);
    let body: Value = merged.json().await.unwrap();
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("different amount"),
        "{body}"
    );

    assert_eq!(accept(&server, &charge, None).await.status(), 200);
    let rows = ledger_rows(&server, &bank).await;
    let mut amounts: Vec<&str> = rows
        .iter()
        .map(|row| row["amount"]["display"].as_str().unwrap())
        .collect();
    amounts.sort_unstable();
    assert_eq!(amounts, ["1.00", "500.00"]);

    // The charge's statement again offers nothing: its fingerprint still knows the line.
    assert_eq!(upload(&server, &bank, "ofx", &levy).await.status(), 200);
    assert_eq!(inbox(&server).await["total"], 0);
}

#[tokio::test]
async fn a_charge_kept_without_its_reference_is_still_recognised_when_it_comes_again() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    upload(
        &server,
        &bank,
        "ofx",
        &ofx_line("TRF-3", "-500.00", "TRANSFER"),
    )
    .await;
    let transfer = inbox(&server).await["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(accept(&server, &transfer, None).await.status(), 200);
    upload(&server, &bank, "ofx", &ofx_line("TRF-3", "-1.00", "E-LEVY")).await;
    let charge = inbox(&server).await["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(accept(&server, &charge, None).await.status(), 200);

    // The same charge in a later export, worded differently, so a new line.
    upload(
        &server,
        &bank,
        "ofx",
        &ofx_line("TRF-3", "-1.00", "E-LEVY CHARGE"),
    )
    .await;
    let again = inbox(&server).await["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let response = accept(&server, &again, None).await;

    assert_eq!(response.status(), 409);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["message"].as_str().unwrap().contains("E-LEVY CHARGE"),
        "{body}"
    );
    assert_eq!(ledger_rows(&server, &bank).await.len(), 2);
}

#[tokio::test]
async fn a_transfer_and_its_charge_accepted_together_are_both_kept() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    upload(
        &server,
        &bank,
        "ofx",
        &ofx_line("TRF-7", "-500.00", "TRANSFER"),
    )
    .await;
    upload(
        &server,
        &bank,
        "ofx",
        &ofx_line("TRF-7", "-1.00", "TRANSFER"),
    )
    .await;
    let waiting = inbox(&server).await;
    let items: Vec<Value> = waiting["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| json!({ "id": item["id"] }))
        .collect();
    assert_eq!(items.len(), 2);

    let response = server
        .post("/inbox/accept", json!({ "items": items }))
        .await;

    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    assert_eq!(ledger_rows(&server, &bank).await.len(), 2);
}

#[tokio::test]
async fn the_same_payment_twice_in_one_request_is_refused_and_writes_nothing() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    // One movement, exported twice with different wording: two lines, one reference.
    upload(
        &server,
        &bank,
        "ofx",
        &ofx_line("REF-5", "-20.00", "SHOPRITE"),
    )
    .await;
    upload(
        &server,
        &bank,
        "ofx",
        &ofx_line("REF-5", "-20.00", "SHOPRITE OSU"),
    )
    .await;
    let waiting = inbox(&server).await;
    let items: Vec<Value> = waiting["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| json!({ "id": item["id"] }))
        .collect();

    let response = server
        .post("/inbox/accept", json!({ "items": items }))
        .await;

    assert_eq!(response.status(), 409);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["message"].as_str().unwrap().contains("one payment"),
        "{body}"
    );
    assert!(ledger_rows(&server, &bank).await.is_empty());
    assert_eq!(inbox(&server).await["total"], 2);
}

#[tokio::test]
async fn a_full_page_of_lines_is_accepted_in_one_request() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    let mut statement = String::from("Date,Description,Amount,Payee\n");
    for day in 0..300 {
        statement.push_str(&format!(
            "{:02}/10/2026,FARE {day},-{}.00,TROTRO\n",
            day % 28 + 1,
            day + 1
        ));
    }
    import(&server, &bank, &statement).await;
    let waiting: Value = server.get("/inbox?limit=500").await.json().await.unwrap();
    assert_eq!(waiting["total"], 300);
    let items: Vec<Value> = waiting["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| json!({ "id": item["id"] }))
        .collect();

    let response = server
        .post("/inbox/accept", json!({ "items": items }))
        .await;

    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let written: Value = response.json().await.unwrap();
    // The inbox lists a page at a time; every line on it is written in the one request.
    let accepted = items.len();
    assert!(accepted >= 200, "{accepted}");
    assert_eq!(written["accepted"], accepted);
    assert_eq!(ledger_rows(&server, &bank).await.len(), accepted);
    assert_eq!(inbox(&server).await["total"], 300 - accepted);
}

#[tokio::test]
async fn a_merchant_filed_two_ways_in_one_request_is_learned_the_last_way() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    import(
        &server,
        &bank,
        "Date,Description,Amount,Payee\n\
         03/10/2026,CARD 1,-30.00,Shoprite\n\
         04/10/2026,CARD 2,-40.00,SHOPRITE\n",
    )
    .await;
    let eating_out = category(&server, "Eating out").await;
    let groceries = category(&server, "Food & groceries").await;
    let waiting = inbox(&server).await;
    let first = item(&waiting, "CARD 1")["id"].clone();
    let second = item(&waiting, "CARD 2")["id"].clone();

    let response = server
        .post(
            "/inbox/accept",
            json!({ "items": [
                { "id": first, "category_id": eating_out },
                { "id": second, "category_id": groceries },
            ] }),
        )
        .await;
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());

    import(
        &server,
        &bank,
        "Date,Description,Amount,Payee\n09/10/2026,CARD 3,-25.00,Shoprite Osu\n",
    )
    .await;
    assert_eq!(
        item(&inbox(&server).await, "CARD 3")["category_id"],
        groceries.as_str()
    );
}

#[tokio::test]
async fn a_rejected_line_stays_rejected_when_the_statement_comes_again() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    import(&server, &bank, STATEMENT).await;
    let waiting = inbox(&server).await;
    let shop = item(&waiting, "POS PURCHASE")["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let rejected = server
        .post(&format!("/inbox/{shop}/reject"), json!({}))
        .await;
    assert_eq!(rejected.status(), 200);
    import(&server, &bank, STATEMENT).await;

    assert_eq!(inbox(&server).await["total"], 1);
}

// ----------------------------------------------------------------------------- tenancy ----

#[tokio::test]
async fn one_persons_inbox_is_not_anothers_to_see_or_settle() {
    let ama = signed_in().await;
    let bank = account(&ama, "GHS").await;
    import(&ama, &bank, STATEMENT).await;
    let waiting = inbox(&ama).await;
    let shop = item(&waiting, "POS PURCHASE")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let ama_row: Value = ama
        .post(
            "/transactions",
            json!({ "account_id": bank, "direction": "out", "amount": "1.00",
                    "label": "Ama's" }),
        )
        .await
        .json()
        .await
        .unwrap();

    let kofi = ama.another_browser();
    kofi.register("kofi@example.test").await;
    let kofi_bank = account(&kofi, "GHS").await;

    assert_eq!(inbox(&kofi).await["total"], 0);
    assert_eq!(accept(&kofi, &shop, None).await.status(), 404);
    let reject = kofi.post(&format!("/inbox/{shop}/reject"), json!({})).await;
    assert_eq!(reject.status(), 404);
    let merge = kofi
        .post(
            &format!("/inbox/{shop}/merge"),
            json!({ "transaction_id": ama_row["id"] }),
        )
        .await;
    assert_eq!(merge.status(), 404);
    assert_eq!(upload(&kofi, &bank, "csv", STATEMENT).await.status(), 404);
    let preview = kofi
        .post(
            "/imports/preview",
            json!({ "account_id": bank, "content_base64": "YQ==" }),
        )
        .await;
    assert_eq!(preview.status(), 404);

    // Kofi's own line cannot be merged into Ama's row either.
    import(&kofi, &kofi_bank, STATEMENT).await;
    let kofi_waiting = inbox(&kofi).await;
    let kofi_shop = item(&kofi_waiting, "POS PURCHASE")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let cross = kofi
        .post(
            &format!("/inbox/{kofi_shop}/merge"),
            json!({ "transaction_id": ama_row["id"] }),
        )
        .await;
    assert_eq!(cross.status(), 404);
    assert_eq!(inbox(&ama).await["total"], 2);
}

// --------------------------------------------------------------- limits and refusals ----

#[tokio::test]
async fn a_statement_of_several_megabytes_uploads_and_one_past_the_limit_is_refused() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    let description = "D".repeat(200);
    let payee = "P".repeat(120);
    let mut text = String::from("Date,Description,Amount,Payee\n");
    for _ in 0..9_000 {
        text.push_str(&format!("03/10/2026,{description},-1.00,{payee}\n"));
    }
    // Over axum's 2 MB default once base64-encoded, so the raised limit is what lets it in.
    assert!(text.len() > 2 * 1024 * 1024);

    let outcome = import(&server, &bank, &text).await;
    assert_eq!(outcome["added"], 9_000);

    let too_big = server
        .post(
            "/imports",
            json!({ "account_id": bank, "format": "csv", "mapping": mapping(),
                    "content_base64": "A".repeat(8 * 1024 * 1024) }),
        )
        .await;
    assert_eq!(too_big.status(), 413);
}

#[tokio::test]
async fn accepting_keeps_the_ordinary_body_limit() {
    let server = signed_in().await;
    let items: Vec<Value> = (0..2_000)
        .map(|_| json!({ "id": uuid::Uuid::now_v7().to_string(), "category_id": null }))
        .collect();

    let response = server
        .post("/inbox/accept", json!({ "items": items }))
        .await;

    assert_eq!(response.status(), 413);
}

#[tokio::test]
async fn an_accept_request_that_makes_no_sense_is_refused_before_anything_is_written() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    import(&server, &bank, STATEMENT).await;
    let waiting = inbox(&server).await;
    let shop = item(&waiting, "POS PURCHASE")["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let empty = server.post("/inbox/accept", json!({ "items": [] })).await;
    assert_eq!(empty.status(), 422);

    let many: Vec<Value> = (0..501)
        .map(|_| json!({ "id": uuid::Uuid::now_v7().to_string() }))
        .collect();
    let too_many = server.post("/inbox/accept", json!({ "items": many })).await;
    assert_eq!(too_many.status(), 422);

    let twice = server
        .post(
            "/inbox/accept",
            json!({ "items": [{ "id": shop }, { "id": shop }] }),
        )
        .await;
    assert_eq!(twice.status(), 422);

    let kofi = server.another_browser();
    kofi.register("kofi@example.test").await;
    let kofis_category = category(&kofi, "Transport").await;
    let not_mine = accept(&server, &shop, Some(&kofis_category)).await;
    assert_eq!(not_mine.status(), 404);

    assert!(ledger_rows(&server, &bank).await.is_empty());
    assert_eq!(inbox(&server).await["total"], 2);
}

#[tokio::test]
async fn a_balance_column_holding_a_card_number_leaves_that_line_out_not_the_file() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    let with_balance = json!({
        "delimiter": "comma", "skip_rows": 0, "has_header": true, "date": 0,
        "date_format": "day_month_year", "amount": { "kind": "signed", "column": 2 },
        "label": 1, "counterparty": null, "reference": null, "balance": 3
    });
    let text = "Date,Description,Amount,Card\n\
                03/10/2026,Card,-5.00,4111111111111111\n\
                04/10/2026,Fare,-2.00,90.00\n\
                05/10/2026,Typo year,-2.00,88.00\n";
    let text = text.replace("05/10/2026", "05/10/0226");

    let response = server
        .post(
            "/imports",
            json!({ "account_id": bank, "format": "csv", "mapping": with_balance,
                    "content_base64": Base64::encode_string(text.as_bytes()) }),
        )
        .await;

    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let outcome: Value = response.json().await.unwrap();
    assert_eq!(outcome["added"], 1);
    assert_eq!(outcome["problem_count"], 2);
}

#[tokio::test]
async fn lines_sharing_a_reference_in_one_statement_can_each_be_accepted() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    let with_reference = json!({
        "delimiter": "comma", "skip_rows": 0, "has_header": true, "date": 0,
        "date_format": "day_month_year", "amount": { "kind": "signed", "column": 2 },
        "label": 1, "counterparty": null, "reference": 3, "balance": null
    });
    let text = "Date,Description,Amount,Ref\n\
                03/10/2026,Transfer to Kofi,-500.00,TRF-1\n\
                03/10/2026,E-levy,-5.00,TRF-1\n";
    server
        .post(
            "/imports",
            json!({ "account_id": bank, "format": "csv", "mapping": with_reference,
                    "content_base64": Base64::encode_string(text.as_bytes()) }),
        )
        .await;
    let waiting = inbox(&server).await;
    let ids: Vec<Value> = waiting["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| json!({ "id": item["id"] }))
        .collect();

    let response = server.post("/inbox/accept", json!({ "items": ids })).await;

    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    assert_eq!(ledger_rows(&server, &bank).await.len(), 2);
}

// ------------------------------------------------------------------ merging, refused ----

async fn typed(server: &TestServer, account: &str, amount: &str, direction: &str) -> String {
    let row: Value = server
        .post(
            "/transactions",
            json!({ "account_id": account, "direction": direction, "amount": amount,
                    "label": "Trotro", "counterparty": "STATION",
                    "booked_at": "2026-10-03T07:00:00Z" }),
        )
        .await
        .json()
        .await
        .unwrap();
    row["id"].as_str().unwrap().to_owned()
}

async fn merge(server: &TestServer, line: &str, into: &str) -> reqwest::Response {
    server
        .post(
            &format!("/inbox/{line}/merge"),
            json!({ "transaction_id": into }),
        )
        .await
}

#[tokio::test]
async fn two_lines_of_one_statement_cannot_both_be_merged_into_one_row() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    let fare = typed(&server, &bank, "5.00", "out").await;
    import(
        &server,
        &bank,
        "Date,Description,Amount,Payee\n\
         03/10/2026,Trotro,-5.00,STATION\n\
         03/10/2026,Trotro,-5.00,STATION\n",
    )
    .await;
    let waiting = inbox(&server).await;
    let first = waiting["items"][0]["id"].as_str().unwrap().to_owned();
    let second = waiting["items"][1]["id"].as_str().unwrap().to_owned();
    assert_eq!(merge(&server, &first, &fare).await.status(), 200);

    let after = inbox(&server).await;
    assert!(
        after["items"][0]["suspects"].as_array().unwrap().is_empty(),
        "a row one line has claimed is not offered to its neighbour: {after}"
    );
    assert_eq!(merge(&server, &second, &fare).await.status(), 409);
}

#[tokio::test]
async fn a_line_is_not_merged_into_a_row_it_cannot_be() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    let wallet = account(&server, "GHS").await;
    import(
        &server,
        &bank,
        "Date,Description,Amount,Payee\n03/10/2026,Trotro,-5.00,STATION\n",
    )
    .await;
    let line = inbox(&server).await["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let elsewhere = typed(&server, &wallet, "5.00", "out").await;
    assert_eq!(merge(&server, &line, &elsewhere).await.status(), 409);
    let other_way = typed(&server, &bank, "5.00", "in").await;
    assert_eq!(merge(&server, &line, &other_way).await.status(), 409);
    let other_amount = typed(&server, &bank, "7.00", "out").await;
    assert_eq!(merge(&server, &line, &other_amount).await.status(), 409);

    let gone = typed(&server, &bank, "5.00", "out").await;
    server.delete(&format!("/transactions/{gone}")).await;
    assert_eq!(merge(&server, &line, &gone).await.status(), 404);

    assert_eq!(inbox(&server).await["total"], 1);
}

#[tokio::test]
async fn the_same_month_uploaded_twice_in_two_forms_is_flagged_while_both_wait() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    import(
        &server,
        &bank,
        "Date,Description,Amount,Payee\n03/10/2026,POS SHOPRITE,-20.00,SHOPRITE\n",
    )
    .await;
    let ofx = "<OFX><STMTRS><CURDEF>GHS<BANKTRANLIST>\
               <STMTTRN><DTPOSTED>20261003<TRNAMT>-20.00<FITID>X1<NAME>SHOPRITE ACCRA\
               </BANKTRANLIST></STMTRS></OFX>";
    assert_eq!(upload(&server, &bank, "ofx", ofx).await.status(), 200);

    let waiting = inbox(&server).await;

    assert_eq!(waiting["total"], 2);
    for line in waiting["items"].as_array().unwrap() {
        assert_eq!(line["twins"].as_array().unwrap().len(), 1, "{line}");
    }
}

#[tokio::test]
async fn the_count_says_how_many_wait_and_for_which_account() {
    let server = signed_in().await;
    let bank = account(&server, "GHS").await;
    import(&server, &bank, STATEMENT).await;

    let count: Value = server.get("/inbox/count").await.json().await.unwrap();

    assert_eq!(count["total"], 2);
    assert_eq!(count["by_account"][0]["account_id"], bank.as_str());
    assert_eq!(count["by_account"][0]["waiting"], 2);
}
