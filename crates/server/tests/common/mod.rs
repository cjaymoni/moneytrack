//! A server, a database of its own, and a browser-shaped client to talk to it.
//!
//! Each test gets its own database, created and dropped around it, so tests can run in
//! parallel and one leaving rows behind cannot change what another sees. The server is
//! started for real on a loopback port and spoken to over HTTP with a cookie jar, because
//! the session and the CSRF token *are* cookies: a test that called the handlers directly
//! would be testing a different program.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::sync::{Arc, Mutex};

use mt_server::config::Config;
use mt_server::limit::Attempts;
use mt_server::{AppState, db, router};
use serde_json::{Value, json};
use uuid::Uuid;

/// Where the tests find a Postgres. The container `tools/pg.sh` starts listens here.
fn admin_url() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://moneytrack:moneytrack@127.0.0.1:55432/moneytrack".to_owned()
    })
}

/// A client that keeps cookies, as a browser does.
///
/// It opens a fresh connection per request rather than reusing one. After refusing an
/// oversized upload the server closes the connection without reading the rest of the body;
/// a pooled client could hand the next request to that closed connection and fail with a
/// reset that has nothing to do with the test. Concurrent requests never share a connection
/// either way.
fn browser() -> reqwest::Client {
    reqwest::Client::builder()
        .cookie_store(true)
        .pool_max_idle_per_host(0)
        .build()
        .unwrap()
}

/// A running server, its database, and a client that keeps cookies.
pub struct TestServer {
    pub base: String,
    pub client: reqwest::Client,
    /// The CSRF token this client holds. `reqwest`'s jar sends the cookie but will not let
    /// us read it, and the whole point of the token is that it is echoed in a header — so
    /// it is captured from `Set-Cookie` the way a script on the page would read it.
    token: Mutex<Option<String>>,
    database: String,
}

impl TestServer {
    /// Starts one.
    pub async fn start() -> Self {
        // Internal errors are logged rather than returned, so a failing test would
        // otherwise say only "the server could not complete that".
        let _ = tracing_subscriber::fmt()
            .with_env_filter("mt_server=debug")
            .with_test_writer()
            .try_init();

        let database = format!("mt_test_{}", Uuid::now_v7().simple());
        run_on_admin(&format!("CREATE DATABASE \"{database}\"")).await;

        let url = swap_database(&admin_url(), &database);
        let pool = db::connect(&url).expect("pool");
        db::migrate(&pool).await.expect("migrate");

        let config = Config {
            database_url: url,
            bind: "127.0.0.1:0".to_owned(),
            // Plain HTTP on loopback: a `Secure` cookie would never be sent and every test
            // would fail for a reason that has nothing to do with what it is testing.
            secure_cookies: false,
            session_ttl: std::time::Duration::from_secs(3600),
        };
        let state = AppState {
            pool,
            config: Arc::new(config),
            attempts: Arc::new(Attempts::default()),
        };

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .ok();
        });

        Self {
            base: format!("http://{address}/api"),
            client: browser(),
            token: Mutex::new(None),
            database,
        }
    }

    /// A second client against the same server: another browser, nobody signed in.
    pub fn another_browser(&self) -> Self {
        Self {
            base: self.base.clone(),
            client: browser(),
            token: Mutex::new(None),
            // Only the browser that created the database drops it.
            database: String::new(),
        }
    }

    /// A GET.
    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap()
    }

    /// A POST carrying the CSRF token this client holds, as a browser would.
    pub async fn post(&self, path: &str, body: Value) -> reqwest::Response {
        if self.csrf_token().is_none() {
            self.prime().await;
        }
        let mut request = self.client.post(format!("{}{path}", self.base)).json(&body);
        if let Some(token) = self.csrf_token() {
            request = request.header("x-csrf-token", token);
        }
        request.send().await.unwrap()
    }

    /// Runs SQL on this server's own database, to set up history the API cannot create —
    /// a budget that began months ago, say. Never used to check what the API did: that is
    /// read back through the API, as a browser would.
    pub async fn run_sql(&self, sql: &str) {
        let url = swap_database(&admin_url(), &self.database);
        let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
            .await
            .expect("connect to the test database");
        let handle = tokio::spawn(async move {
            connection.await.ok();
        });
        client.batch_execute(sql).await.expect("run test setup");
        drop(client);
        handle.abort();
    }

    /// A connection of its own to this server's database, for holding a lock while a request
    /// runs — to show the request waits for it. Never used to check what the API did.
    pub async fn side_connection(&self) -> tokio_postgres::Client {
        let url = swap_database(&admin_url(), &self.database);
        let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
            .await
            .expect("connect to the test database");
        tokio::spawn(async move {
            connection.await.ok();
        });
        client
    }

    /// A PATCH carrying the CSRF token.
    pub async fn patch(&self, path: &str, body: Value) -> reqwest::Response {
        self.with_token(
            self.client
                .patch(format!("{}{path}", self.base))
                .json(&body),
        )
        .await
    }

    /// A DELETE carrying the CSRF token.
    pub async fn delete(&self, path: &str) -> reqwest::Response {
        self.with_token(self.client.delete(format!("{}{path}", self.base)))
            .await
    }

    /// Sends a request with this client's CSRF token, fetching one first if it has none.
    async fn with_token(&self, request: reqwest::RequestBuilder) -> reqwest::Response {
        if self.csrf_token().is_none() {
            self.prime().await;
        }
        let request = match self.csrf_token() {
            Some(token) => request.header("x-csrf-token", token),
            None => request,
        };
        request.send().await.unwrap()
    }

    /// A POST with no CSRF header, as a cross-site form would send.
    pub async fn post_without_csrf(&self, path: &str, body: Value) -> reqwest::Response {
        self.client
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    /// Reads the CSRF cookie the server issued.
    fn csrf_token(&self) -> Option<String> {
        self.token.lock().ok().and_then(|held| held.clone())
    }

    /// Registers a person and signs them in. Returns their email.
    pub async fn register(&self, email: &str) -> String {
        // A GET first, which is how a browser gets its CSRF token before posting anything.
        self.prime().await;
        let response = self
            .post(
                "/auth/register",
                json!({ "email": email, "password": "a long enough password" }),
            )
            .await;
        assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
        email.to_owned()
    }

    /// Fetches the CSRF token the way a browser does: by making a request.
    pub async fn prime(&self) {
        let response = self.get("/health").await;
        if let Some(token) = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find_map(|value| value.strip_prefix("mt_csrf="))
            .and_then(|value| value.split(';').next())
        {
            if let Ok(mut held) = self.token.lock() {
                *held = Some(token.to_owned());
            }
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if self.database.is_empty() {
            return;
        }
        let database = self.database.clone();
        // Best effort: a leftover test database is untidy, not wrong, and panicking in a
        // destructor would hide whatever the test actually found.
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            if let Ok(runtime) = runtime {
                runtime.block_on(async {
                    run_on_admin(&format!(
                        "DROP DATABASE IF EXISTS \"{database}\" WITH (FORCE)"
                    ))
                    .await;
                });
            }
        })
        .join()
        .ok();
    }
}

/// Runs one statement on the server's own maintenance database.
async fn run_on_admin(sql: &str) {
    let (client, connection) = tokio_postgres::connect(&admin_url(), tokio_postgres::NoTls)
        .await
        .expect("connect to postgres: is the container running? see tools/pg.sh");
    let handle = tokio::spawn(async move {
        connection.await.ok();
    });
    client
        .batch_execute(sql)
        .await
        .expect("run admin statement");
    drop(client);
    handle.abort();
}

/// Points a connection string at another database on the same server.
fn swap_database(url: &str, database: &str) -> String {
    match url.rsplit_once('/') {
        Some((head, _)) => format!("{head}/{database}"),
        None => url.to_owned(),
    }
}
