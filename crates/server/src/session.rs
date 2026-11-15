//! Sessions: how the server knows who is asking.
//!
//! An opaque token, not a JWT. The browser is handed random bytes in a cookie it cannot read
//! from JavaScript, and the server keeps only a SHA-256 of them. That buys three things a
//! signed token does not: a leaked database hands over no live session, signing out is
//! immediate rather than "when the token expires", and there is no signature algorithm to
//! get wrong.
//!
//! What this does *not* do, and the schema allows for: rotate on every use. A browser makes
//! requests in parallel from several tabs, and rotating per request turns two simultaneous
//! requests into a stolen-token alarm. Rotation belongs to the mobile refresh flow, which is
//! why `refresh_tokens.rotated_from` exists; here a session is minted at sign-in and revoked
//! at sign-out or on a password change.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use base64ct::{Base64UrlUnpadded, Encoding};
use deadpool_postgres::Pool;
use mt_core::ids::UserId;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::ApiError;
use crate::{AppState, db};

/// The cookie the session token travels in.
pub const COOKIE: &str = "mt_session";

/// The path cookies are scoped to. Everything this server answers is under it, and nothing
/// else on the domain — a static client, a marketing page — ever receives the session.
pub const COOKIE_PATH: &str = "/api";

/// Bytes of token. 256 bits from the OS generator; not a UUID, which is a name rather than
/// a secret.
const TOKEN_BYTES: usize = 32;

/// Who is asking. Extracted from the session cookie, or the request is refused.
#[derive(Debug, Clone, Copy)]
pub struct Session {
    /// The signed-in user.
    pub user: UserId,
    /// The session row, so signing out can revoke exactly this one.
    pub id: Uuid,
}

impl FromRequestParts<AppState> for Session {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar.get(COOKIE).ok_or(ApiError::Unauthenticated)?;
        lookup(&state.pool, token.value()).await
    }
}

/// Finds the session a token names, if it is still live.
///
/// The lookup is by hash, so a token that is not in the database is indistinguishable from
/// one that never existed. Expiry and revocation are checked in the query rather than after
/// it, so there is no branch here that could forget one.
///
/// # Errors
/// [`ApiError::Unauthenticated`] if there is no live session for this token.
async fn lookup(pool: &Pool, token: &str) -> Result<Session, ApiError> {
    let digest = fingerprint(token);
    let client = db::client(pool).await?;
    let found = client
        .query_opt(
            "SELECT s.id, s.user_id
               FROM refresh_tokens s
               JOIN users u ON u.id = s.user_id
              WHERE s.token_hash = $1
                AND s.revoked_at IS NULL
                AND s.expires_at > now()
                AND u.deleted_at IS NULL",
            &[&digest.as_slice()],
        )
        .await
        .map_err(|error| ApiError::database("read a session", &error))?;

    let row = found.ok_or(ApiError::Unauthenticated)?;
    let id: Uuid = row
        .try_get("id")
        .map_err(|error| ApiError::database("read a session", &error))?;
    let user: Uuid = row
        .try_get("user_id")
        .map_err(|error| ApiError::database("read a session", &error))?;
    Ok(Session {
        id,
        user: UserId::from(user),
    })
}

/// Mints a session for a user and returns the token to hand to the browser.
///
/// # Errors
/// [`ApiError::Internal`] if the generator or the insert fails.
pub async fn issue(
    pool: &Pool,
    user: UserId,
    ttl: std::time::Duration,
) -> Result<String, ApiError> {
    let token = secret()?;
    let expires_at = OffsetDateTime::now_utc()
        .checked_add(
            time::Duration::try_from(ttl).map_err(|_| ApiError::Internal {
                operation: "compute a session expiry",
                reason: "the configured session lifetime is not representable".to_owned(),
            })?,
        )
        .ok_or(ApiError::Internal {
            operation: "compute a session expiry",
            reason: "the expiry falls outside the calendar".to_owned(),
        })?;

    // The session carries no device row of its own: the browser's device is found or
    // created once, and minting one per sign-in would fill that table with rows nothing
    // ever reads.
    let client = db::client(pool).await?;
    client
        .execute(
            "INSERT INTO refresh_tokens (id, user_id, device_id, token_hash, expires_at)
             VALUES ($1, $2, NULL, $3, $4)",
            &[
                &Uuid::now_v7(),
                user.as_uuid(),
                &fingerprint(&token).as_slice(),
                &expires_at,
            ],
        )
        .await
        .map_err(|error| ApiError::database("issue a session", &error))?;

    Ok(token)
}

/// Revokes one session. Signing out twice is not an error.
///
/// # Errors
/// [`ApiError::Internal`] if the update fails.
pub async fn revoke(pool: &Pool, id: Uuid) -> Result<(), ApiError> {
    let client = db::client(pool).await?;
    client
        .execute(
            "UPDATE refresh_tokens SET revoked_at = now()
              WHERE id = $1 AND revoked_at IS NULL",
            &[&id],
        )
        .await
        .map(|_| ())
        .map_err(|error| ApiError::database("revoke a session", &error))
}

/// Revokes every session a user has. What a password change does.
///
/// # Errors
/// [`ApiError::Internal`] if the update fails.
pub async fn revoke_all(pool: &Pool, user: UserId) -> Result<(), ApiError> {
    let client = db::client(pool).await?;
    client
        .execute(
            "UPDATE refresh_tokens SET revoked_at = now()
              WHERE user_id = $1 AND revoked_at IS NULL",
            &[user.as_uuid()],
        )
        .await
        .map(|_| ())
        .map_err(|error| ApiError::database("revoke every session", &error))
}

/// The cookie that carries a session.
pub fn cookie(token: String, secure: bool, ttl: std::time::Duration) -> Cookie<'static> {
    let mut cookie = Cookie::new(COOKIE, token);
    // HttpOnly so no script can read it, which is what keeps an XSS from becoming a stolen
    // session that outlives the fix. Lax rather than Strict so following a link into the app
    // does not look signed out; the CSRF token covers what Lax does not.
    cookie.set_http_only(true);
    cookie.set_secure(secure);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_path(COOKIE_PATH);
    cookie.set_max_age(time::Duration::try_from(ttl).ok());
    cookie
}

/// The cookie that clears a session.
pub fn cleared(secure: bool) -> Cookie<'static> {
    let mut cookie = Cookie::new(COOKIE, "");
    cookie.set_http_only(true);
    cookie.set_secure(secure);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_path(COOKIE_PATH);
    cookie.set_max_age(time::Duration::ZERO);
    cookie
}

/// Random bytes, base64url, from the OS generator.
///
/// # Errors
/// [`ApiError::Internal`] if the OS generator fails, which is not a thing to paper over.
pub fn secret() -> Result<String, ApiError> {
    let mut bytes = [0u8; TOKEN_BYTES];
    // Straight from the OS. Not a pseudo-random generator seeded once and reused, and
    // emphatically not a UUID: a token is a secret, and a UUID is a name.
    getrandom::fill(&mut bytes).map_err(|error| ApiError::Internal {
        operation: "generate a secret",
        reason: error.to_string(),
    })?;
    Ok(Base64UrlUnpadded::encode_string(&bytes))
}

/// What is stored in place of a token.
fn fingerprint(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}
