//! Cross-site request forgery.
//!
//! The session cookie is `SameSite=Lax`, which stops a cross-site form POST from carrying
//! it. That is a browser's promise rather than ours, it has exceptions, and the subject here
//! is someone's money — so there is a token as well.
//!
//! Double submit, with one wrinkle worth the trouble: the token cookie is issued on *any*
//! request that arrives without one, not at sign-in. Issuing it at sign-in would leave the
//! sign-in itself unprotected, and login CSRF — logging a victim into the attacker's
//! account, so their next entry lands in it — is a real attack on a finance app rather than
//! a theoretical one. The client reads the cookie and echoes it in a header, which a
//! cross-site caller cannot do: it can send the cookie, but it cannot read it.
//!
//! "Reads the cookie" needs one more step in a browser. The cookie is scoped to `/api` like
//! the session, and `document.cookie` on a page at `/` does not include a cookie scoped to a
//! path the page is not under. So [`token`] hands the same value back in a response body.
//! That gives an attacker nothing the cookie did not: a cross-site page can make the request,
//! but the same-origin policy stops it reading the answer, which is the property the cookie
//! relied on too.

use axum::Json;
use axum::extract::{Extension, Request, State};
use axum::http::{HeaderValue, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use subtle::ConstantTimeEq;

use crate::AppState;
use crate::error::ApiError;
use crate::session;

/// The cookie the token travels in. Readable by script, unlike the session: that is the
/// whole mechanism.
pub const COOKIE: &str = "mt_csrf";

/// The header the client echoes it in.
pub const HEADER: &str = "x-csrf-token";

/// Checks the token on anything that changes state, and issues one to anyone without it.
///
/// # Errors
/// [`ApiError::BadCsrf`] if a state-changing request arrives without a matching pair.
pub async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let jar = CookieJar::from_headers(request.headers());
    let held = jar.get(COOKIE).map(|cookie| cookie.value().to_owned());

    if changes_state(request.method()) {
        let sent = request
            .headers()
            .get(HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);

        let matched = match (&held, &sent) {
            // Constant time, so a wrong token cannot be found a character at a time.
            (Some(cookie), Some(header)) if !cookie.is_empty() => {
                cookie.as_bytes().ct_eq(header.as_bytes()).into()
            }
            _ => false,
        };
        if !matched {
            return ApiError::BadCsrf.into_response();
        }
    }

    let mut request = request;
    let issued = match &held {
        Some(_) => None,
        None => match session::secret() {
            Ok(token) => Some(token),
            // Without a token the next state-changing request is refused, which is the safe
            // direction to fail in. Worth a line in the log, not a failed read.
            Err(error) => {
                tracing::error!(%error, "could not issue a CSRF token");
                None
            }
        },
    };
    if let Some(token) = held.as_ref().or(issued.as_ref()) {
        request.extensions_mut().insert(Token(token.clone()));
    }

    let mut response = next.run(request).await;
    if let Some(token) = issued {
        attach(&mut response, token, state.config.secure_cookies);
    }
    response
}

/// The token this request holds, or the one being issued with its response.
#[derive(Debug, Clone)]
pub struct Token(String);

/// Hands the client its token, which it cannot read from a cookie scoped to `/api`.
///
/// # Errors
/// [`ApiError::Internal`] if no token could be issued.
pub async fn token(token: Option<Extension<Token>>) -> Result<Json<serde_json::Value>, ApiError> {
    match token {
        Some(Extension(Token(token))) => Ok(Json(serde_json::json!({ "token": token }))),
        None => Err(ApiError::Internal {
            operation: "hand out a CSRF token",
            reason: "none was held or issued".to_owned(),
        }),
    }
}

/// Whether a method is one that may change something.
const fn changes_state(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// Puts a freshly minted token on the response.
fn attach(response: &mut Response, token: String, secure: bool) {
    let mut cookie = Cookie::new(COOKIE, token);
    // Not HttpOnly, deliberately and unavoidably: the client has to read this one to echo
    // it. It is not a credential — on its own it authenticates nothing.
    cookie.set_http_only(false);
    cookie.set_secure(secure);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_path(session::COOKIE_PATH);

    match HeaderValue::from_str(&cookie.to_string()) {
        Ok(value) => {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
        Err(error) => tracing::error!(%error, "could not set the CSRF cookie"),
    }
}
