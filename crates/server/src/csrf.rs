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

use axum::extract::{Request, State};
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

    let mut response = next.run(request).await;
    if held.is_none() {
        match session::secret() {
            Ok(token) => attach(&mut response, token, state.config.secure_cookies),
            // Without a token the next state-changing request is refused, which is the safe
            // direction to fail in. Worth a line in the log, not a failed read.
            Err(error) => tracing::error!(%error, "could not issue a CSRF token"),
        }
    }
    response
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
