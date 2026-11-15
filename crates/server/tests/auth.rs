//! Registering, signing in, and the two things that guard every request.

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

#[tokio::test]
async fn registering_signs_you_in() {
    let server = TestServer::start().await;

    server.register("ama@example.test").await;
    let me = server.get("/me").await;

    assert_eq!(me.status(), 200);
    let body: Value = me.json().await.unwrap();
    assert_eq!(body["email"], "ama@example.test");
}

#[tokio::test]
async fn an_address_can_only_be_registered_once() {
    let server = TestServer::start().await;
    server.register("kofi@example.test").await;

    let again = server
        .post(
            "/auth/register",
            json!({ "email": "kofi@example.test", "password": "a long enough password" }),
        )
        .await;

    assert_eq!(again.status(), 409);
    let body: Value = again.json().await.unwrap();
    assert_eq!(body["code"], "conflict");
}

#[tokio::test]
async fn a_short_password_is_refused_and_says_so() {
    let server = TestServer::start().await;
    server.prime().await;

    let response = server
        .post(
            "/auth/register",
            json!({ "email": "yaa@example.test", "password": "short" }),
        )
        .await;

    assert_eq!(response.status(), 422);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "invalid");
    assert!(
        body["message"].as_str().unwrap_or_default().contains("12"),
        "the refusal does not say how long it has to be: {body}"
    );
}

#[tokio::test]
async fn an_address_that_is_not_one_is_refused() {
    let server = TestServer::start().await;
    server.prime().await;

    let response = server
        .post(
            "/auth/register",
            json!({ "email": "not an address", "password": "a long enough password" }),
        )
        .await;

    assert_eq!(response.status(), 422);
}

#[tokio::test]
async fn signing_in_again_works_and_a_wrong_password_does_not() {
    let server = TestServer::start().await;
    server.register("esi@example.test").await;

    let wrong = server
        .post(
            "/auth/login",
            json!({ "email": "esi@example.test", "password": "not the password" }),
        )
        .await;
    assert_eq!(wrong.status(), 401);

    let right = server
        .post(
            "/auth/login",
            json!({ "email": "esi@example.test", "password": "a long enough password" }),
        )
        .await;
    assert_eq!(right.status(), 200);
}

#[tokio::test]
async fn an_unknown_address_is_refused_exactly_like_a_wrong_password() {
    let server = TestServer::start().await;
    server.register("known@example.test").await;

    let unknown = server
        .post(
            "/auth/login",
            json!({ "email": "nobody@example.test", "password": "a long enough password" }),
        )
        .await;
    let wrong = server
        .post(
            "/auth/login",
            json!({ "email": "known@example.test", "password": "not the password" }),
        )
        .await;

    // The same status and the same body, so the sign-in form cannot be used to find out
    // who has an account here.
    assert_eq!(unknown.status(), wrong.status());
    let unknown: Value = unknown.json().await.unwrap();
    let wrong: Value = wrong.json().await.unwrap();
    assert_eq!(unknown, wrong);
}

#[tokio::test]
async fn signing_out_ends_the_session() {
    let server = TestServer::start().await;
    server.register("sena@example.test").await;

    let out = server.post("/auth/logout", json!({})).await;
    assert_eq!(out.status(), 200);

    assert_eq!(server.get("/me").await.status(), 401);
}

#[tokio::test]
async fn nobody_in_particular_cannot_read_anything() {
    let server = TestServer::start().await;

    assert_eq!(server.get("/me").await.status(), 401);
    assert_eq!(server.get("/accounts").await.status(), 401);
    assert_eq!(server.get("/transactions").await.status(), 401);
}

#[tokio::test]
async fn a_state_changing_request_without_the_csrf_token_is_refused() {
    let server = TestServer::start().await;
    server.register("adjoa@example.test").await;

    // A cross-site form can make the browser send the cookies. It cannot read them, so it
    // cannot echo the token in a header — which is the whole mechanism.
    let response = server
        .post_without_csrf(
            "/accounts",
            json!({ "name": "Wallet", "kind": "momo", "currency": "GHS",
                    "opening_balance_minor": 0 }),
        )
        .await;

    assert_eq!(response.status(), 403);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["code"], "bad_csrf");
}

#[tokio::test]
async fn health_says_nothing_about_the_database() {
    let server = TestServer::start().await;

    let response = server.get("/health").await;

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}
