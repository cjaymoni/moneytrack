import { ApiError } from "../api/client";

/** Something is on its way. Quiet on purpose: most loads here take less time than reading this. */
export function Loading({ full = false }: { full?: boolean }) {
  return (
    <div className={full ? "state state--full" : "state"} role="status" aria-live="polite">
      <span className="mt-label">Loading</span>
    </div>
  );
}

/** Something failed. Says what, in the server's words where it gave some, and offers again. */
export function Failure({
  error,
  retry,
  full = false,
}: {
  error: unknown;
  retry?: () => void;
  full?: boolean;
}) {
  const message =
    error instanceof ApiError ? error.message : "Something went wrong showing this.";
  return (
    <div className={full ? "state state--full" : "state"} role="alert">
      <p className="error-line">{message}</p>
      {retry && (
        <button type="button" className="btn btn--ghost" onClick={retry}>
          Try again
        </button>
      )}
    </div>
  );
}

/** The refusal under a form: an oxblood rule and the server's own sentence. */
export function FormError({ error }: { error: unknown }) {
  if (!error) return null;
  const message =
    error instanceof ApiError ? sentence(error) : "Something went wrong. Try again.";
  return (
    <p className="error-line" role="alert">
      {message}
    </p>
  );
}

/** Turns a refusal into what a person should read. */
function sentence(error: ApiError): string {
  switch (error.code) {
    case "bad_credentials":
      return "That email and password do not match.";
    case "conflict":
      return capitalise(error.message) + ".";
    case "invalid":
      return capitalise(error.message.replace(/_/g, " ")) + ".";
    default:
      return error.message;
  }
}

function capitalise(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}
