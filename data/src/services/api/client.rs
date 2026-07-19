use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use parking_lot::{Mutex, RwLock};
use reqwest::{Client, Method, StatusCode};
use tracing::{debug, warn};
use url::Url;

use crate::types::error::NokkviError;

/// Callback invoked when a refreshed JWT is received from the server.
/// Called with the new token string so callers can persist it to redb.
pub type TokenRefreshCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// Persistence policy: write the rotated JWT to redb only when the
/// currently-stored token's remaining lifetime drops below a safety margin
/// derived from the *new* token's full lifetime. Expresses the actual
/// invariant — "the stored token must not be unusably close to expiry on
/// resume" — without committing to any magic wall-clock constant that would
/// need re-tuning per Navidrome `SessionTimeout`.
///
/// The margin is a 3-way clamp:
///
/// 1. **Preferred**: `PERSIST_LIFETIME_PCT %` of the rotated token's full
///    lifetime. Adapts naturally across `SessionTimeout` configurations
///    spanning many orders of magnitude.
/// 2. **Floor**: at least `MIN_MARGIN_FLOOR_SECS`. Protects users on shorter
///    `SessionTimeout` from a "close nokkvi, reopen tomorrow" failure
///    where the stored token has rotated in memory but not been written —
///    guaranteeing at least this much grace on close.
/// 3. **Ceiling**: at most `MAX_MARGIN_LIFETIME_PCT %` of the lifetime.
///    Protects users on very short `SessionTimeout` (e.g., 5 min) from the
///    floor producing a margin larger than the token's own lifetime, which
///    would force a persist on every rotation.
///
/// Concrete behavior across realistic `SessionTimeout` values:
///
/// | SessionTimeout |  Margin | ~persists per active hour |
/// | ---           |  ---    | ---                       |
/// | 5 min          |  150 s  | ~24                       |
/// | 1 hour         |  30 min | ~2                        |
/// | 48 hours (default) | 24 h | ~0.04 (one per day)      |
/// | 8760 hours (1 year) | 36.5 d | ~0.0001 (one per year) |
///
/// Empirically validated against a live Navidrome (`SessionTimeout = 8760h`):
/// the server signs `now + SessionTimeout` deterministically, so concurrent
/// identical requests within the same wall-second receive identical rotated
/// tokens — the dedup branch below collapses those bursts to a single
/// persistence-decision pass without the policy ever firing twice.
const PERSIST_LIFETIME_PCT: i64 = 10;
const MIN_MARGIN_FLOOR_SECS: i64 = 24 * 3600;
const MAX_MARGIN_LIFETIME_PCT: i64 = 50;

pub struct ApiClient {
    client: Arc<Client>,
    base_url: Url,
    /// JWT token, wrapped in RwLock for interior mutability.
    /// Updated transparently by response interceptor when Navidrome returns
    /// a refreshed token via the `X-ND-Authorization` header.
    token: Arc<RwLock<String>>,
    /// Optional callback invoked when token is refreshed, for persistence.
    on_token_refresh: Option<TokenRefreshCallback>,
    /// Unix-seconds `exp` claim of the token currently in redb. Compared
    /// against the rotated token's `exp` on every refresh to decide whether
    /// to persist. Shared across `Clone` so every route through this client
    /// agrees on what's stored. `None` means "we couldn't decode the initial
    /// token" — the next rotation that *can* be decoded will persist and
    /// seed this.
    persisted_exp: Arc<Mutex<Option<i64>>>,
}

impl ApiClient {
    pub fn new(base_url: Url, token: String) -> Self {
        // Configure client with shorter idle timeout for faster shutdown
        let client = Client::builder()
            .user_agent(crate::USER_AGENT)
            .pool_idle_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("Failed to build HTTP client");

        // Seed `persisted_exp` from the token we were constructed with. On a
        // fresh login the saved redb token == this token, so reading its exp
        // gives us the authoritative "what's currently persisted" timestamp.
        // On a logout/empty-string init, decode fails and we leave None.
        let initial_exp = decode_jwt_exp(&token).ok();

        Self {
            client: Arc::new(client),
            base_url,
            token: Arc::new(RwLock::new(token)),
            on_token_refresh: None,
            persisted_exp: Arc::new(Mutex::new(initial_exp)),
        }
    }

    /// Set a callback to be invoked when a refreshed JWT is received.
    /// Used to persist the new token to redb.
    pub fn set_on_token_refresh(&mut self, callback: TokenRefreshCallback) {
        self.on_token_refresh = Some(callback);
    }
}

impl Clone for ApiClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            base_url: self.base_url.clone(),
            token: self.token.clone(),
            on_token_refresh: self.on_token_refresh.clone(),
            persisted_exp: self.persisted_exp.clone(),
        }
    }
}

/// The per-process client unique id sent as `X-ND-Client-Unique-Id` on
/// every native request AND the SSE connection — Navidrome uses it to skip
/// echoing a client's own mutations back over the event stream. Minted
/// once from the existing `rand` dependency (no `uuid` crate exists in the
/// workspace and none is added).
pub fn client_unique_id() -> &'static str {
    use std::sync::OnceLock;
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        use rand::RngExt;
        let mut rng = rand::rng();
        (0..16)
            .map(|_| format!("{:x}", rng.random_range(0..16)))
            .collect()
    })
}

/// Decode a JWT's payload segment to JSON. Does NOT verify the signature —
/// the server already verified the token by accepting the request.
fn decode_jwt_payload(jwt: &str) -> Result<serde_json::Value> {
    let payload = jwt
        .split('.')
        .nth(1)
        .context("token is not in JWT header.payload.signature form")?;
    let bytes = decode_base64url(payload).context("JWT payload is not valid base64url")?;
    serde_json::from_slice(&bytes).context("JWT payload is not valid JSON")
}

/// Decode a JWT's `exp` claim (RFC 7519, registered claim). Returns the
/// unix-seconds timestamp as i64.
fn decode_jwt_exp(jwt: &str) -> Result<i64> {
    decode_jwt_payload(jwt)?
        .get("exp")
        .and_then(|x| x.as_i64())
        .context("JWT payload has no numeric `exp` claim")
}

/// Decode a Navidrome JWT's `uid` claim — the authenticated user's id
/// (`reference-navidrome/core/auth/claims.go`: `m["uid"] = c.UserID`). Used
/// by session resume, where no login response carries the id; the playlist
/// ownership gate (`owner_id == session user_id`) depends on it.
pub(crate) fn decode_jwt_uid(jwt: &str) -> Result<String> {
    decode_jwt_payload(jwt)?
        .get("uid")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .context("JWT payload has no string `uid` claim")
}

/// Minimal base64url decoder (RFC 4648 §5, no padding). Inlined to avoid
/// pulling `base64` in as a direct workspace dependency for this single use.
fn decode_base64url(input: &str) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for byte in input.bytes() {
        let v: u32 = match byte {
            b'A'..=b'Z' => (byte - b'A') as u32,
            b'a'..=b'z' => (byte - b'a' + 26) as u32,
            b'0'..=b'9' => (byte - b'0' + 52) as u32,
            b'-' => 62,
            b'_' => 63,
            b'=' => continue,
            _ => return Err(anyhow!("invalid base64url byte: 0x{byte:02x}")),
        };
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// Body-read policy for [`ApiClient::execute`], chosen per HTTP verb.
///
/// `Required` (GET/POST/PUT): callers parse the body, so a failed read is a
/// hard error. `Tolerant` (DELETE): callers discard the body — the old
/// per-verb delete applied the status policy without ever reading the body
/// on 401/2xx and only best-effort-read it (`unwrap_or_default`) for the
/// error message — so a failed read degrades to an empty string and the
/// status alone decides the outcome. A `Required` DELETE would mask the
/// 401 → `Unauthorized` downcast and turn a successful delete into an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodyPolicy {
    Required,
    Tolerant,
}

/// Content type for an image filename, by extension (case-insensitive).
/// Covers the four formats Navidrome's image-upload endpoints decode
/// (jpeg/png/gif/webp); anything else degrades to `application/octet-stream`
/// — the server sniffs the actual bytes, so the mime is advisory.
fn mime_for_image_filename(filename: &str) -> &'static str {
    let ext = filename
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Current wall-clock time as unix seconds. Saturates to 0 on a time-travel
/// system clock (pre-epoch) — same fallback as redb / serde defaults.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl ApiClient {
    /// Get the underlying HTTP client for making raw requests
    pub fn http_client(&self) -> Arc<Client> {
        self.client.clone()
    }

    /// Get the current bearer token string (read lock)
    fn bearer_header(&self) -> String {
        format!("Bearer {}", self.token.read())
    }

    /// Check response headers for a refreshed JWT and update if found.
    /// Navidrome's JWTRefresher middleware returns a fresh token in the
    /// `X-ND-Authorization` header on every authenticated response.
    fn intercept_token_refresh(&self, response: &reqwest::Response) {
        let Some(header_value) = response.headers().get("x-nd-authorization") else {
            return;
        };
        let Ok(new_token) = header_value.to_str() else {
            return;
        };
        // Strip "Bearer " prefix if present
        let token_str = new_token.strip_prefix("Bearer ").unwrap_or(new_token);
        self.apply_refreshed_token(token_str);
    }

    /// Apply a refreshed token: atomically dedup-check and swap the in-memory
    /// value, then decide whether to persist based on how close the
    /// currently-stored token is to its `exp`.
    ///
    /// Split out from `intercept_token_refresh` so the dedup + persistence
    /// policy is unit-testable without constructing a `reqwest::Response`.
    fn apply_refreshed_token(&self, token_str: &str) {
        // Atomic dedup + swap under a single write lock. The previous
        // implementation took a read lock for the dedup check, dropped it,
        // then acquired a separate write lock — a TOCTOU race where N
        // concurrent responses each saw the same "stale" current value and
        // each fired the persistence callback.
        let changed = {
            let mut current = self.token.write();
            if *current == token_str {
                false
            } else {
                *current = token_str.to_string();
                true
            }
        };
        if !changed {
            return;
        }

        debug!("JWT refreshed from server response header");

        // Decide persistence by comparing the *stored* token's remaining
        // lifetime against a fraction of the *new* token's full lifetime.
        // Both numbers come from the JWTs themselves, so the policy
        // self-adapts to the server's `SessionTimeout` without any
        // client-side magic constant.
        let new_exp = match decode_jwt_exp(token_str) {
            Ok(e) => e,
            Err(e) => {
                // We can't reason about lifetimes without `exp`. Preserve
                // whatever is currently in redb (which we know decoded fine
                // at construction time, or is whatever a previous successful
                // rotation persisted) rather than overwriting it with a
                // token whose claims we can't parse.
                warn!("Could not decode refreshed JWT exp claim: {e}; skipping persistence");
                return;
            }
        };

        let now = unix_now();
        // `new_lifetime` is the rotated token's full validity window, used
        // as the basis for the safety margin. `.max(1)` guards a corner case
        // where the server's clock thinks the token already expired
        // (shouldn't happen in practice; defensive).
        let new_lifetime = (new_exp - now).max(1);
        let preferred = (new_lifetime * PERSIST_LIFETIME_PCT) / 100;
        let ceiling = (new_lifetime * MAX_MARGIN_LIFETIME_PCT) / 100;
        // Clamp order: take the larger of preferred / floor (we want at
        // least the floor of safety), then cap at the ceiling (we don't want
        // the floor to exceed half the token's own lifetime).
        let margin = preferred.max(MIN_MARGIN_FLOOR_SECS).min(ceiling);

        let should_persist = {
            let mut persisted = self.persisted_exp.lock();
            let stored_remaining = persisted.map(|p| p - now).unwrap_or(0);
            if stored_remaining < margin {
                *persisted = Some(new_exp);
                true
            } else {
                false
            }
        };
        if !should_persist {
            return;
        }

        if let Some(ref callback) = self.on_token_refresh {
            callback(token_str);
        }
    }

    /// Build an authenticated request for the Navidrome REST API: join the
    /// endpoint onto the base URL, append query params, and attach the
    /// `X-ND-Authorization` bearer header.
    fn build_request(
        &self,
        method: Method,
        endpoint: &str,
        params: &[(&str, &str)],
    ) -> Result<reqwest::RequestBuilder> {
        let mut url = self
            .base_url
            .join(endpoint)
            .with_context(|| format!("Failed to join endpoint: {endpoint}"))?;

        // Build query string manually to avoid Send issues with query_pairs_mut
        if !params.is_empty() {
            let mut query_parts = Vec::new();
            for (key, value) in params {
                query_parts.push(format!(
                    "{}={}",
                    url::form_urlencoded::byte_serialize(key.as_bytes()).collect::<String>(),
                    url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
                ));
            }
            url.set_query(Some(&query_parts.join("&")));
        }

        Ok(self
            .client
            .request(method, url.as_str())
            .header("X-ND-Authorization", self.bearer_header())
            // Navidrome skips echoing our own mutations over SSE for
            // requests carrying the same unique id as the event-stream
            // connection — the draft-workspace hardening layer.
            .header("X-ND-Client-Unique-Id", client_unique_id()))
    }

    /// Apply the shared response status policy: 2xx returns the body, 401
    /// routes to [`NokkviError::Unauthorized`] (the UI's session-expiry
    /// downcast depends on it), 403 routes to [`NokkviError::Forbidden`]
    /// (permission failures like artwork-upload-disabled map to friendly
    /// toasts), anything else becomes a descriptive error.
    ///
    /// Pure (no I/O) so it can be unit-tested without an HTTP server —
    /// mirrors `check_subsonic_response_status` in
    /// [`crate::services::api::subsonic`].
    fn finish_status(status: StatusCode, body: String, ctx: &str) -> Result<String> {
        if status.is_success() {
            Ok(body)
        } else if status == StatusCode::UNAUTHORIZED {
            Err(NokkviError::Unauthorized.into())
        } else if status == StatusCode::FORBIDDEN {
            // Keep the generic arm's detail text inside the typed variant so
            // log lines lose nothing to the split.
            Err(NokkviError::Forbidden(format!("{ctx} failed with status {status}: {body}")).into())
        } else {
            Err(anyhow!("{ctx} failed with status {status}: {body}"))
        }
    }

    /// Resolve a body-read result against the verb's [`BodyPolicy`]. Pure
    /// (no I/O) so the required/tolerant split is unit-testable without an
    /// HTTP server.
    fn resolve_body(policy: BodyPolicy, read: Result<String>) -> Result<String> {
        match policy {
            BodyPolicy::Required => read.context("Failed to read response body"),
            BodyPolicy::Tolerant => Ok(read.unwrap_or_default()),
        }
    }

    /// Send a built request and run the shared response pipeline: intercept
    /// the refreshed JWT on EVERY response (including 401/5xx — JWT rotation
    /// must not stop on error-heavy sessions), snapshot the `X-Total-Count`
    /// header before the body read consumes the response, then apply the
    /// status policy.
    async fn execute(
        &self,
        request: reqwest::RequestBuilder,
        ctx: &str,
        body_policy: BodyPolicy,
    ) -> Result<(String, Option<u32>)> {
        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to send {ctx} request"))?;

        // Intercept refreshed JWT from response header
        self.intercept_token_refresh(&response);

        let status = response.status();

        // Extract X-Total-Count header if present
        let total_count = response
            .headers()
            .get("X-Total-Count")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());

        let body = Self::resolve_body(body_policy, response.text().await.map_err(Into::into))?;

        Ok((Self::finish_status(status, body, ctx)?, total_count))
    }

    /// Make a GET request to the Navidrome REST API.
    /// `endpoint`: API path (e.g., "/api/album").
    /// `params`: query parameters as key-value pairs.
    ///
    /// Thin wrapper around [`Self::get_with_headers`] that discards the
    /// `X-Total-Count` header — use `get_with_headers` directly when the
    /// total is needed for pagination.
    pub async fn get(&self, endpoint: &str, params: &[(&str, &str)]) -> Result<String> {
        let (body, _total_count) = self.get_with_headers(endpoint, params).await?;
        Ok(body)
    }

    /// Make a GET request and return both body and headers
    /// Returns (body, total_count_from_header)
    pub async fn get_with_headers(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
    ) -> Result<(String, Option<u32>)> {
        let request = self.build_request(Method::GET, endpoint, params)?;
        self.execute(
            request,
            &format!("API GET {endpoint}"),
            BodyPolicy::Required,
        )
        .await
    }

    /// Make a POST request with a JSON body to the Navidrome REST API
    pub async fn post_json(
        &self,
        endpoint: &str,
        json_body: &impl serde::Serialize,
    ) -> Result<String> {
        let request = self
            .build_request(Method::POST, endpoint, &[])?
            .json(json_body);
        let (body, _total_count) = self
            .execute(
                request,
                &format!("API POST {endpoint}"),
                BodyPolicy::Required,
            )
            .await?;
        Ok(body)
    }

    /// Make a PUT request with a JSON body to the Navidrome REST API
    pub async fn put_json(
        &self,
        endpoint: &str,
        json_body: &impl serde::Serialize,
    ) -> Result<String> {
        let request = self
            .build_request(Method::PUT, endpoint, &[])?
            .json(json_body);
        let (body, _total_count) = self
            .execute(
                request,
                &format!("API PUT {endpoint}"),
                BodyPolicy::Required,
            )
            .await?;
        Ok(body)
    }

    /// Make a `multipart/form-data` POST to the Navidrome REST API with a
    /// single file part named `field_name` (Navidrome's image-upload endpoints
    /// expect exactly one part literally named `image`). Routes through the
    /// same [`Self::build_request`] + [`Self::execute`] pipeline as every
    /// other verb, so JWT attachment, token-refresh interception, and the
    /// 401/403 status mapping keep working.
    pub async fn post_multipart(
        &self,
        endpoint: &str,
        field_name: &'static str,
        bytes: Vec<u8>,
        filename: &str,
    ) -> Result<String> {
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(filename.to_string())
            .mime_str(mime_for_image_filename(filename))
            .context("failed to build multipart file part")?;
        let form = reqwest::multipart::Form::new().part(field_name, part);
        let request = self
            .build_request(Method::POST, endpoint, &[])?
            .multipart(form);
        let (body, _total_count) = self
            .execute(
                request,
                &format!("API POST {endpoint}"),
                BodyPolicy::Required,
            )
            .await?;
        Ok(body)
    }

    #[cfg(test)]
    pub(crate) fn current_token(&self) -> String {
        self.token.read().clone()
    }

    #[cfg(test)]
    pub(crate) fn set_persisted_exp(&self, exp: Option<i64>) {
        *self.persisted_exp.lock() = exp;
    }

    #[cfg(test)]
    pub(crate) fn persisted_exp_snapshot(&self) -> Option<i64> {
        *self.persisted_exp.lock()
    }

    /// Make a DELETE request to the Navidrome REST API. The body is
    /// discarded, so it's read under [`BodyPolicy::Tolerant`] — a failed
    /// body read must not turn a successful delete into an error nor mask
    /// the 401 → `Unauthorized` downcast.
    pub async fn delete(&self, endpoint: &str) -> Result<()> {
        self.delete_with_params(endpoint, &[]).await.map(|_body| ())
    }

    /// DELETE with query parameters, returning the response body — the
    /// playlist track-removal path inspects the echoed id (the
    /// silent-no-op tripwire). Same pipeline and body policy as
    /// [`Self::delete`]; an empty body on a 2xx surfaces as `Ok("")`, which
    /// echo-checking callers treat as failure.
    pub async fn delete_with_params(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
    ) -> Result<String> {
        let request = self.build_request(Method::DELETE, endpoint, params)?;
        let (body, _) = self
            .execute(
                request,
                &format!("API DELETE {endpoint}"),
                BodyPolicy::Tolerant,
            )
            .await?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    /// Tiny base64url encoder for the test-fixture path only. Mirrors RFC
    /// 4648 §5 (no padding), same alphabet as `decode_base64url`.
    fn encode_base64url(input: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        let mut buf: u32 = 0;
        let mut bits: u32 = 0;
        for &b in input {
            buf = (buf << 8) | b as u32;
            bits += 8;
            while bits >= 6 {
                bits -= 6;
                out.push(ALPHABET[((buf >> bits) & 0x3F) as usize] as char);
            }
        }
        if bits > 0 {
            out.push(ALPHABET[((buf << (6 - bits)) & 0x3F) as usize] as char);
        }
        out
    }

    /// Build a minimal JWT-shaped string with the given `exp`. Only the
    /// payload's `exp` claim is consulted by `decode_jwt_exp`; header and
    /// signature are placeholders.
    fn jwt_with_exp(exp: i64) -> String {
        let payload = serde_json::json!({ "exp": exp });
        let payload_b64 = encode_base64url(payload.to_string().as_bytes());
        format!("hdr.{payload_b64}.sig")
    }

    #[test]
    fn base64url_roundtrip_matches_inline_decoder() {
        // The fixture encoder must produce bytes the production decoder
        // can read back; otherwise jwt_with_exp tests pass for the wrong
        // reason. Cover a few edge lengths (1, 2, 3-byte tails).
        for sample in [
            b"".as_slice(),
            b"x",
            b"xy",
            b"xyz",
            b"hello world",
            br#"{"exp":1234567890}"#,
        ] {
            let encoded = encode_base64url(sample);
            let decoded = decode_base64url(&encoded).unwrap();
            assert_eq!(decoded, sample);
        }
    }

    fn make_client(token: &str) -> ApiClient {
        let url = Url::parse("http://example.test/").unwrap();
        ApiClient::new(url, token.to_string())
    }

    fn counting_callback() -> (Arc<AtomicUsize>, TokenRefreshCallback) {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_cb = counter.clone();
        let cb: TokenRefreshCallback = Arc::new(move |_| {
            counter_cb.fetch_add(1, Ordering::SeqCst);
        });
        (counter, cb)
    }

    #[test]
    fn refresh_with_new_token_updates_in_memory_token() {
        let (_counter, cb) = counting_callback();
        let stored = jwt_with_exp(unix_now() + 3600);
        let mut client = make_client(&stored);
        client.set_on_token_refresh(cb);

        let rotated = jwt_with_exp(unix_now() + 3600 + 60);
        client.apply_refreshed_token(&rotated);

        // In-memory token swaps regardless of persistence decision so
        // subsequent outgoing requests use the freshest credential.
        assert_eq!(client.current_token(), rotated);
    }

    #[test]
    fn refresh_with_same_token_skips_callback_and_keeps_stored_exp() {
        let (counter, cb) = counting_callback();
        let exp = unix_now() + 3600;
        let token = jwt_with_exp(exp);
        let mut client = make_client(&token);
        client.set_on_token_refresh(cb);

        client.apply_refreshed_token(&token);

        assert_eq!(counter.load(Ordering::SeqCst), 0, "dedup must hold");
        assert_eq!(client.persisted_exp_snapshot(), Some(exp));
    }

    #[test]
    fn fresh_stored_token_skips_persist() {
        // 48 h SessionTimeout, stored token has the full window remaining —
        // far inside the safety margin (10 % of 48 h = 4.8 h).
        let (counter, cb) = counting_callback();
        let now = unix_now();
        let stored = jwt_with_exp(now + 48 * 3600);
        let mut client = make_client(&stored);
        client.set_on_token_refresh(cb);

        // Server rotates: new token has full lifetime, stored still has
        // nearly full lifetime. Policy: stored_remaining >> margin → no
        // persist.
        let rotated = jwt_with_exp(now + 48 * 3600 + 1);
        client.apply_refreshed_token(&rotated);

        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "healthy stored token must not trigger a persist"
        );
    }

    #[test]
    fn stale_stored_token_triggers_persist_and_updates_persisted_exp() {
        // 48 h SessionTimeout, stored token has 1 h remaining (well below
        // the 24 h floor margin).
        let (counter, cb) = counting_callback();
        let now = unix_now();
        let stored = jwt_with_exp(now + 3600);
        let mut client = make_client(&stored);
        client.set_on_token_refresh(cb);

        let new_exp = now + 48 * 3600;
        let rotated = jwt_with_exp(new_exp);
        client.apply_refreshed_token(&rotated);

        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "near-expiry stored token must trigger a persist"
        );
        assert_eq!(
            client.persisted_exp_snapshot(),
            Some(new_exp),
            "persisted_exp must reflect the freshly-written token"
        );
    }

    #[test]
    fn default_48h_session_close_grace_floor_kicks_in() {
        // 48 h SessionTimeout, stored token has 23 h remaining — under the
        // 24 h floor (the bare 10 % rule would only require 4.8 h margin
        // and would skip this persist, leaving the user with <24 h of
        // stored grace at app-close time).
        let (counter, cb) = counting_callback();
        let now = unix_now();
        let stored = jwt_with_exp(now + 23 * 3600);
        let mut client = make_client(&stored);
        client.set_on_token_refresh(cb);

        let new_exp = now + 48 * 3600;
        client.apply_refreshed_token(&jwt_with_exp(new_exp));

        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "23 h remaining on a 48 h session must persist (floor = 24 h)"
        );
    }

    #[test]
    fn default_48h_session_above_floor_skips_persist() {
        // Just above the 24 h floor — confirms we don't persist on every
        // rotation for default-config users.
        let (counter, cb) = counting_callback();
        let now = unix_now();
        let stored = jwt_with_exp(now + 25 * 3600);
        let mut client = make_client(&stored);
        client.set_on_token_refresh(cb);

        client.apply_refreshed_token(&jwt_with_exp(now + 48 * 3600));

        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "25 h remaining on a 48 h session is above the 24 h floor"
        );
    }

    #[test]
    fn short_session_timeout_ceiling_prevents_persist_storm() {
        // 5-minute SessionTimeout. Without the 50 % ceiling, the 24 h floor
        // would exceed the entire token lifetime, forcing a persist on
        // every rotation. With the ceiling, margin = 50 % * 300 s = 150 s.
        // Stored token at 200 s remaining → above 150 s → no persist.
        let (counter, cb) = counting_callback();
        let now = unix_now();
        let stored = jwt_with_exp(now + 200);
        let mut client = make_client(&stored);
        client.set_on_token_refresh(cb);

        client.apply_refreshed_token(&jwt_with_exp(now + 300));

        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "200 s > 150 s margin (50 % of 300 s) — no persist on 5 min session"
        );
    }

    #[test]
    fn short_session_timeout_below_ceiling_persists() {
        // Same 5-minute SessionTimeout, but stored has 100 s remaining —
        // below the 150 s ceiling-clamped margin.
        let (counter, cb) = counting_callback();
        let now = unix_now();
        let stored = jwt_with_exp(now + 100);
        let mut client = make_client(&stored);
        client.set_on_token_refresh(cb);

        client.apply_refreshed_token(&jwt_with_exp(now + 300));

        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "100 s remaining < 150 s margin — must persist"
        );
    }

    #[test]
    fn very_long_session_timeout_persists_rarely() {
        // 8760 h (1 year) SessionTimeout. Stored token has 364 d remaining —
        // 10 % margin is ~36.5 d. 364 d > 36.5 d → no persist.
        let (counter, cb) = counting_callback();
        let now = unix_now();
        let stored = jwt_with_exp(now + 364 * 86400);
        let mut client = make_client(&stored);
        client.set_on_token_refresh(cb);

        let rotated = jwt_with_exp(now + 365 * 86400);
        client.apply_refreshed_token(&rotated);

        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "year-long session must not persist on steady-state rotation"
        );
    }

    #[test]
    fn decode_failure_preserves_existing_persisted_exp() {
        // Stored token decodes fine and seeds persisted_exp. The rotated
        // value is malformed — policy is to leave the stored value alone
        // rather than overwrite with something we can't reason about.
        let (counter, cb) = counting_callback();
        let stored_exp = unix_now() + 3600;
        let stored = jwt_with_exp(stored_exp);
        let mut client = make_client(&stored);
        client.set_on_token_refresh(cb);

        client.apply_refreshed_token("not-a-jwt");

        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "undecodable refreshed token must not fire the persistence callback"
        );
        assert_eq!(
            client.persisted_exp_snapshot(),
            Some(stored_exp),
            "persisted_exp must remain anchored to the last good token"
        );
        // In-memory token still updates so the next outgoing request at
        // least gets a chance — if the token really is broken, the next
        // 401 will trigger re-auth.
        assert_eq!(client.current_token(), "not-a-jwt");
    }

    #[test]
    fn concurrent_burst_with_identical_rotated_token_persists_once() {
        // Mirrors the empirically-observed Navidrome behavior: 64 concurrent
        // requests with the same input token all receive the *same* rotated
        // token (per-second deterministic signing). The atomic dedup-and-
        // swap means thread-1 wins the swap, the other 63 see `current ==
        // token_str` and short-circuit.
        let now = unix_now();
        // Stored is stale enough that the FIRST thread will persist.
        let stored = jwt_with_exp(now + 60);
        let (counter, cb) = counting_callback();
        let mut client = make_client(&stored);
        client.set_on_token_refresh(cb);
        let client = Arc::new(client);

        let rotated = jwt_with_exp(now + 48 * 3600);
        let mut handles = Vec::new();
        for _ in 0..64 {
            let c = client.clone();
            let t = rotated.clone();
            handles.push(std::thread::spawn(move || c.apply_refreshed_token(&t)));
        }
        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "64 concurrent identical-token refreshes must collapse to one persist"
        );
    }

    #[test]
    fn unparseable_initial_token_leaves_persisted_exp_unseeded() {
        // Construction with an empty / malformed token leaves persisted_exp
        // as None — the first decodable rotation will then unconditionally
        // persist (stored_remaining defaults to 0, < any positive margin)
        // and seed the field.
        let (counter, cb) = counting_callback();
        let mut client = make_client("");
        assert_eq!(client.persisted_exp_snapshot(), None);
        client.set_on_token_refresh(cb);

        let now = unix_now();
        let rotated = jwt_with_exp(now + 48 * 3600);
        client.apply_refreshed_token(&rotated);

        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(client.persisted_exp_snapshot(), Some(now + 48 * 3600));
    }

    #[test]
    fn set_persisted_exp_helper_round_trips() {
        // Sanity check on the test helper itself.
        let client = make_client(&jwt_with_exp(unix_now() + 60));
        client.set_persisted_exp(Some(12345));
        assert_eq!(client.persisted_exp_snapshot(), Some(12345));
        client.set_persisted_exp(None);
        assert_eq!(client.persisted_exp_snapshot(), None);
    }

    #[test]
    fn finish_status_success_returns_body() {
        let body =
            ApiClient::finish_status(StatusCode::OK, "payload".to_string(), "API GET /api/song")
                .expect("2xx must return the body");
        assert_eq!(body, "payload");
    }

    /// HTTP 401 must downcast to [`NokkviError::Unauthorized`] so the UI's
    /// session-expiry path (`session_expired_message`) drops to login —
    /// mirrors `check_subsonic_response_status_routes_401_to_unauthorized`.
    #[test]
    fn finish_status_401_downcasts_to_unauthorized() {
        let err =
            ApiClient::finish_status(StatusCode::UNAUTHORIZED, String::new(), "API GET /api/song")
                .expect_err("401 must produce an error");

        let nokkvi_err = err
            .downcast_ref::<NokkviError>()
            .expect("401 should downcast to NokkviError");
        assert!(
            matches!(nokkvi_err, NokkviError::Unauthorized),
            "expected NokkviError::Unauthorized, got {nokkvi_err:?}"
        );
    }

    /// HTTP 403 must downcast to [`NokkviError::Forbidden`] so the artwork
    /// upload/reset handlers can map permission failures to a friendly toast,
    /// while the detail string keeps the ctx + status + body for logs.
    #[test]
    fn finish_status_403_downcasts_to_forbidden_with_detail() {
        let err = ApiClient::finish_status(
            StatusCode::FORBIDDEN,
            "not authorized".to_string(),
            "API POST /api/playlist/p1/image",
        )
        .expect_err("403 must produce an error");

        let nokkvi_err = err
            .downcast_ref::<NokkviError>()
            .expect("403 should downcast to NokkviError");
        let NokkviError::Forbidden(detail) = nokkvi_err else {
            panic!("expected NokkviError::Forbidden, got {nokkvi_err:?}");
        };
        assert!(detail.contains("API POST /api/playlist/p1/image"));
        assert!(detail.contains("403"));
        assert!(detail.contains("not authorized"));
        // The Display marker the flattened string matcher keys on.
        assert!(NokkviError::is_forbidden_str(&format!("{err:#}")));
        assert!(!NokkviError::is_unauthorized_str(&format!("{err:#}")));
    }

    #[test]
    fn mime_for_image_filename_maps_known_extensions() {
        assert_eq!(mime_for_image_filename("cover.png"), "image/png");
        assert_eq!(mime_for_image_filename("cover.jpg"), "image/jpeg");
        assert_eq!(mime_for_image_filename("cover.JPEG"), "image/jpeg");
        assert_eq!(mime_for_image_filename("art.gif"), "image/gif");
        assert_eq!(mime_for_image_filename("art.WebP"), "image/webp");
        // Dotted names resolve by the LAST extension segment.
        assert_eq!(mime_for_image_filename("a.b.c.png"), "image/png");
    }

    #[test]
    fn mime_for_image_filename_falls_back_to_octet_stream() {
        assert_eq!(
            mime_for_image_filename("noextension"),
            "application/octet-stream"
        );
        assert_eq!(mime_for_image_filename(""), "application/octet-stream");
        assert_eq!(
            mime_for_image_filename("weird.bmp"),
            "application/octet-stream"
        );
    }

    #[test]
    fn finish_status_500_includes_ctx_status_body() {
        let err = ApiClient::finish_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server boom".to_string(),
            "API GET /api/song",
        )
        .expect_err("500 must produce an error");

        // Not a NokkviError — caller's downcast for Unauthorized must miss.
        assert!(err.downcast_ref::<NokkviError>().is_none());

        let msg = format!("{err}");
        assert!(msg.contains("API GET /api/song"), "missing ctx in: {msg}");
        assert!(msg.contains("500"), "missing status in: {msg}");
        assert!(msg.contains("server boom"), "missing body in: {msg}");
    }

    #[test]
    fn resolve_body_tolerant_swallows_read_failure() {
        // DELETE's policy: a failed body read degrades to an empty string so
        // the status policy alone decides the outcome (2xx stays Ok, 401
        // stays Unauthorized) — never an error in its own right.
        let body = ApiClient::resolve_body(BodyPolicy::Tolerant, Err(anyhow!("connection reset")))
            .expect("tolerant policy must not propagate a body-read failure");
        assert_eq!(body, "");
    }

    #[test]
    fn resolve_body_required_propagates_read_failure() {
        let err = ApiClient::resolve_body(BodyPolicy::Required, Err(anyhow!("connection reset")))
            .expect_err("required policy must propagate a body-read failure");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("Failed to read response body"),
            "missing context in: {msg}"
        );
    }

    #[test]
    fn resolve_body_passes_through_successful_reads() {
        for policy in [BodyPolicy::Required, BodyPolicy::Tolerant] {
            let body = ApiClient::resolve_body(policy, Ok("payload".to_string()))
                .expect("successful reads pass through under either policy");
            assert_eq!(body, "payload");
        }
    }

    /// With the tolerant (empty) body, `finish_status` reproduces the old
    /// per-verb DELETE error arm exactly: ctx + status, empty body tail.
    /// (2xx → Ok and 401 → Unauthorized with an empty body are covered by
    /// `finish_status_success_returns_body` /
    /// `finish_status_401_downcasts_to_unauthorized` above.)
    #[test]
    fn finish_status_with_empty_body_keeps_delete_error_shape() {
        let err = ApiClient::finish_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            String::new(),
            "API DELETE /api/playlist/1",
        )
        .expect_err("5xx must produce an error");

        assert!(err.downcast_ref::<NokkviError>().is_none());
        let msg = format!("{err}");
        assert!(
            msg.contains("API DELETE /api/playlist/1"),
            "missing ctx in: {msg}"
        );
        assert!(msg.contains("500"), "missing status in: {msg}");
    }

    #[test]
    fn build_request_encodes_params_and_sets_auth_header() {
        let client = make_client("tok");
        let request = client
            .build_request(Method::GET, "/api/song", &[("title", "a b"), ("q", "x&y")])
            .expect("endpoint must join onto the base url")
            .build()
            .expect("request must build");

        // byte_serialize encoding: space becomes '+', '&' becomes %26.
        assert_eq!(request.url().query(), Some("title=a+b&q=x%26y"));

        let auth = request
            .headers()
            .get("X-ND-Authorization")
            .and_then(|v| v.to_str().ok())
            .expect("auth header must be set");
        assert!(auth.starts_with("Bearer "), "got: {auth}");
    }
}
