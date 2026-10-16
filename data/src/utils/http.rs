//! The starting point for the HTTP clients that talk to the server and the
//! web, so a new client cannot drift from its neighbours by copying one of
//! them. Each site adds what only it needs on top: a redirect policy
//! (`utils::redirects`), a read or total timeout sized for what it fetches, a
//! pool idle time, its own user agent. The audio clients (the decoder and the
//! range reader) keep their own, tighter bounds.

use std::time::Duration;

/// A host that went away fails the connect in this long, not the OS's two
/// minutes of SYN retries, so a loader or a star gets its error instead of
/// hanging.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

/// TCP keepalive, so a socket that died under a sleep/resume is noticed.
pub const TCP_KEEPALIVE: Duration = Duration::from_secs(30);

/// A builder with the user agent, the connect timeout and keepalive. Read
/// timeouts stay per site: a large first page can take the server seconds.
pub fn client_base() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .tcp_keepalive(TCP_KEEPALIVE)
}
