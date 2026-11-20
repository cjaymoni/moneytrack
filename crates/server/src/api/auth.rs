//! Registering, signing in, signing out, and asking who you are.

use axum::Json;
use axum::extract::{ConnectInfo, State};
use axum::response::{IntoResponse, Response};
use axum_extra::extract::CookieJar;
use std::net::SocketAddr;

use crate::api::dto::{Credentials, UserOut};
use crate::error::{ApiError, ApiResult};
use crate::repo::{categories, users};
use crate::session::{self, Session};
use crate::{AppState, password};

/// The longest an address may be, matching the schema.
const MAX_EMAIL: usize = 320;

/// A hash of nothing in particular, verified against when no user matches.
///
/// Without it, a sign-in for an address that does not exist returns in microseconds while a
/// real one takes the time Argon2id takes, and the difference tells a caller which addresses
/// are registered. Generated once at first use, from a password nobody holds.
static DECOY: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Registers a person and signs them in.
///
/// # Errors
/// [`ApiError::Invalid`] for an unusable address or password, [`ApiError::Conflict`] if the
/// address is taken, or [`ApiError::Internal`].
pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(caller): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Json(body): Json<Credentials>,
) -> ApiResult<Response> {
    if !state.attempts.allow(caller.ip()) {
        return Err(ApiError::Conflict {
            reason: "too many attempts; wait a minute and try again".to_owned(),
        });
    }

    let email = check_email(&body.email)?;
    let hash = password::hash(&body.password)?;
    let user = users::create(&state.pool, &email, &hash).await?;
    // The device row every write from this browser is attributed to. Created here so the
    // first transaction does not have to.
    let device = users::web_device(&state.pool, user.id).await?;
    // A ledger with no categories makes the first entry ask a question nobody can answer
    // yet. Seeded in a transaction of its own and idempotent, so a failure here leaves a
    // registered person with no categories rather than half of them.
    categories::seed(&state.pool, user.id, device).await?;

    sign_in(&state, jar, user).await
}

/// Signs a person in.
///
/// # Errors
/// [`ApiError::BadCredentials`] if the pair does not match — the same answer whether the
/// address exists or not.
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(caller): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Json(body): Json<Credentials>,
) -> ApiResult<Response> {
    if !state.attempts.allow(caller.ip()) {
        return Err(ApiError::BadCredentials);
    }

    let found = users::by_email(&state.pool, body.email.trim()).await?;
    let (user, stored) = match found {
        Some(pair) => pair,
        None => {
            // Verified against a decoy so the answer takes as long as a real one would.
            let decoy = DECOY.get_or_init(|| {
                password::hash("a password nobody holds, hashed once").unwrap_or_default()
            });
            if !decoy.is_empty() {
                let _ = password::verify(&body.password, decoy)?;
            }
            return Err(ApiError::BadCredentials);
        }
    };

    if !password::verify(&body.password, &stored)? {
        return Err(ApiError::BadCredentials);
    }
    users::web_device(&state.pool, user.id).await?;
    sign_in(&state, jar, user).await
}

/// Signs out, revoking this session and no other.
///
/// # Errors
/// [`ApiError::Internal`] if the revocation fails.
pub async fn logout(
    State(state): State<AppState>,
    session: Session,
    jar: CookieJar,
) -> ApiResult<Response> {
    session::revoke(&state.pool, session.id).await?;
    let jar = jar.add(session::cleared(state.config.secure_cookies));
    Ok((jar, Json(serde_json::json!({ "signed_out": true }))).into_response())
}

/// Who is signed in.
///
/// # Errors
/// [`ApiError::Unauthenticated`] if there is no live session.
pub async fn me(State(state): State<AppState>, session: Session) -> ApiResult<Json<UserOut>> {
    let user = users::by_id(&state.pool, session.user).await?;
    Ok(Json(UserOut::try_from(user)?))
}

/// Mints a session and hands back the cookie with it.
async fn sign_in(
    state: &AppState,
    jar: CookieJar,
    user: crate::repo::users::User,
) -> ApiResult<Response> {
    let token = session::issue(&state.pool, user.id, state.config.session_ttl).await?;
    let jar = jar.add(session::cookie(
        token,
        state.config.secure_cookies,
        state.config.session_ttl,
    ));
    Ok((jar, Json(UserOut::try_from(user)?)).into_response())
}

/// Checks an address far enough to store it. The schema checks it again.
///
/// Deliberately not a regular expression trying to implement RFC 5322: the only honest test
/// of an address is sending mail to it, and everything stricter than this rejects somebody's
/// real address.
fn check_email(supplied: &str) -> ApiResult<String> {
    let email = supplied.trim();
    let refuse = |reason: &str| ApiError::Invalid {
        field: "email".to_owned(),
        reason: reason.to_owned(),
    };

    if email.len() > MAX_EMAIL {
        return Err(refuse("it is too long"));
    }
    let mut parts = email.split('@');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(local), Some(domain), None)
            if !local.is_empty()
                && !domain.is_empty()
                && !email.chars().any(char::is_whitespace) =>
        {
            Ok(email.to_owned())
        }
        _ => Err(refuse("it does not look like an email address")),
    }
}
