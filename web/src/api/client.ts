// The one place this client talks to the server.
//
// Same origin, always: the page and `/api` sit behind one host (Vite's proxy in
// development), so the session cookie travels without CORS and without the client ever
// seeing it. The session is `HttpOnly`; nothing here can read it, which is the point.

/** A refusal, carrying the server's stable code. Screens switch on `code`, never on text. */
export class ApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}

const BASE = "/api";

let csrf: Promise<string> | null = null;

/**
 * The CSRF token, fetched once and held for the page's life.
 *
 * The cookie it lives in is scoped to `/api`, so `document.cookie` on this page cannot see
 * it; the server hands the same value back from `/api/csrf` instead.
 */
function csrfToken(): Promise<string> {
  if (!csrf) {
    csrf = request<{ token: string }>("GET", "/csrf").then(
      (body) => body.token,
      (error: unknown) => {
        csrf = null;
        throw error;
      },
    );
  }
  return csrf;
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = { accept: "application/json" };
  if (body !== undefined) headers["content-type"] = "application/json";
  if (method !== "GET") headers["x-csrf-token"] = await csrfToken();

  let response: Response;
  try {
    response = await fetch(BASE + path, {
      method,
      headers,
      credentials: "same-origin",
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
  } catch {
    throw new ApiError(0, "offline", "Could not reach the server. Check the connection.");
  }

  const text = await response.text();
  let parsed: unknown = null;
  if (text) {
    try {
      parsed = JSON.parse(text);
    } catch {
      parsed = null;
    }
  }

  if (!response.ok) {
    const refusal = parsed as { code?: unknown; message?: unknown } | null;
    throw new ApiError(
      response.status,
      typeof refusal?.code === "string" ? refusal.code : "unknown",
      typeof refusal?.message === "string" ? refusal.message : "The server could not complete that.",
    );
  }
  return parsed as T;
}

/** A read. */
export function get<T>(path: string): Promise<T> {
  return request<T>("GET", path);
}

/**
 * A write, with the CSRF token.
 *
 * A `bad_csrf` refusal is retried once with a fresh token: the cookie can be cleared or
 * expire under a page that is still open, and the person should not have to reload to
 * record a row. Anything else is the caller's to show.
 */
export function post<T>(path: string, body: unknown): Promise<T> {
  return write<T>("POST", path, body);
}

/** A change to an existing thing, sent whole. Retried on `bad_csrf` like `post`. */
export function patch<T>(path: string, body: unknown): Promise<T> {
  return write<T>("PATCH", path, body);
}

/** A deletion. Retried on `bad_csrf` like `post`. */
export function del<T>(path: string): Promise<T> {
  return write<T>("DELETE", path);
}

async function write<T>(method: string, path: string, body?: unknown): Promise<T> {
  try {
    return await request<T>(method, path, body);
  } catch (error) {
    if (error instanceof ApiError && error.code === "bad_csrf") {
      csrf = null;
      return request<T>(method, path, body);
    }
    throw error;
  }
}

/** For tests: forget the held token. */
export function resetCsrf(): void {
  csrf = null;
}
