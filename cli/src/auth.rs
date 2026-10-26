//! `code login`, `logout`, and `auth status`.
//!
//! Login is generic OAuth (authorization code + PKCE). The endpoints and client
//! ID come from `settings.oauth.<provider>` in the user-level settings only, so no
//! vendor client is baked into the binary and a repository cannot redirect a login. Tokens are stored per provider in
//! `credentials.json` and are never printed.

use std::io::{self, BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use code_api::{complete_oauth_login, validate_oauth_endpoint};
use code_runtime::{
    clear_oauth_credentials_for, credentials_key_for_provider, generate_pkce_pair, generate_state,
    load_oauth_credentials_for, loopback_redirect_uri, parse_oauth_callback_query,
    parse_oauth_callback_request_target, ConfigLoader, OAuthAuthorizationRequest,
    OAuthCallbackParams, OAuthConfig, OAuthTokenExchangeRequest,
};

/// Providers that can be logged in to. Each must have a `settings.oauth.<name>` entry.
pub const SUPPORTED_PROVIDERS: &[&str] = &["anthropic", "openai"];

/// How long the local callback server waits for the browser redirect.
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

pub fn validate_provider(provider: &str) -> Result<(), String> {
    if SUPPORTED_PROVIDERS.contains(&provider) {
        Ok(())
    } else {
        Err(format!(
            "unknown provider `{provider}`; expected one of: {}",
            SUPPORTED_PROVIDERS.join(", ")
        ))
    }
}

/// Runs the browser login for `provider` and saves the resulting tokens.
pub fn login(provider: &str, cwd: &Path) -> Result<String, String> {
    validate_provider(provider)?;
    let config = load_provider_oauth_config(cwd, provider)?;
    // Both endpoints receive the user's login (the code and verifier go to the token
    // endpoint), so refuse anything that is not https or exact loopback.
    validate_oauth_endpoint(&config.authorize_url).map_err(|error| error.to_string())?;
    validate_oauth_endpoint(&config.token_url).map_err(|error| error.to_string())?;
    let pkce = generate_pkce_pair().map_err(|error| error.to_string())?;
    let state = generate_state().map_err(|error| error.to_string())?;

    // A configured manual redirect means the provider sends the user to a page
    // we cannot receive on localhost, so the user pastes the final URL back.
    let (redirect_uri, listener) = if let Some(url) = config.manual_redirect_url.clone() {
        (url, None)
    } else {
        let port = config.callback_port.ok_or_else(|| {
            format!(
                "settings.oauth.{provider} needs `callbackPort` (or `manualRedirectUrl`) \
                     so login has a redirect to receive"
            )
        })?;
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|error| format!("could not listen on 127.0.0.1:{port}: {error}"))?;
        (loopback_redirect_uri(port), Some(listener))
    };

    let authorize_url =
        OAuthAuthorizationRequest::from_config(&config, redirect_uri.clone(), state.clone(), &pkce)
            .build_url();
    println!("Sign in to {provider} in your browser:\n{authorize_url}");
    if let Err(error) = open_in_browser(&authorize_url) {
        // Shared fallback: warns on stderr and prints the URL for manual use.
        crate::emit_login_browser_open_failure(
            crate::CliOutputFormat::Text,
            &authorize_url,
            &error,
            &mut io::stdout(),
            &mut io::stderr(),
        )
        .map_err(|error| error.to_string())?;
    }

    let callback = match listener {
        Some(listener) => wait_for_callback(&listener)?,
        None => read_pasted_callback()?,
    };
    if let Some(error) = callback.error.as_deref() {
        let description = callback.error_description.as_deref().unwrap_or("");
        return Err(format!("login was not completed: {error} {description}")
            .trim_end()
            .to_string());
    }
    // Fail closed: the state must match what this process generated.
    if callback.state.as_deref() != Some(state.as_str()) {
        return Err("login failed: OAuth state did not match; try again".to_string());
    }
    let code = callback.code.ok_or_else(|| {
        "login failed: the redirect did not include an authorization code".to_string()
    })?;

    let exchange =
        OAuthTokenExchangeRequest::from_config(&config, code, state, pkce.verifier, redirect_uri);
    complete_oauth_login(provider, &config, &exchange).map_err(|error| error.to_string())?;
    Ok(format!("Logged in to {provider}. Credentials saved."))
}

/// Removes the saved login for `provider`. Environment API keys are unaffected.
pub fn logout(provider: &str) -> Result<String, String> {
    validate_provider(provider)?;
    clear_oauth_credentials_for(credentials_key_for_provider(provider))
        .map_err(|error| error.to_string())?;
    Ok(format!("Logged out of {provider}."))
}

/// Summarizes how each provider is authenticated. Never prints secret values.
pub fn status() -> Result<String, String> {
    let mut lines = Vec::new();
    for provider in SUPPORTED_PROVIDERS {
        let mut sources = Vec::new();
        for var in env_vars_for(provider) {
            if std::env::var(var).is_ok_and(|value| !value.is_empty()) {
                sources.push(format!("env {var}"));
            }
        }
        if let Some(token_set) = load_oauth_credentials_for(credentials_key_for_provider(provider))
            .map_err(|error| error.to_string())?
        {
            sources.push(match token_set.expires_at {
                Some(expires_at) => format!("saved login ({})", describe_expiry(expires_at)),
                None => "saved login".to_string(),
            });
        }
        let summary = if sources.is_empty() {
            "not configured".to_string()
        } else {
            sources.join(", ")
        };
        lines.push(format!("{provider}: {summary}"));
    }
    Ok(lines.join("\n"))
}

fn env_vars_for(provider: &str) -> &'static [&'static str] {
    match provider {
        "anthropic" => &["FRONTAL_API_KEY", "FRONTAL_AUTH_TOKEN"],
        "openai" => &["OPENAI_API_KEY"],
        _ => &[],
    }
}

/// Reads the login endpoints from user-level settings only. Project settings are not
/// consulted, so a repository cannot send the user's authorization code elsewhere.
fn load_provider_oauth_config(cwd: &Path, provider: &str) -> Result<OAuthConfig, String> {
    ConfigLoader::default_for(cwd)
        .user_oauth_config(provider)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            format!(
                "no OAuth config for `{provider}` in your user settings (~/.frontal-code/settings.json). \
                 Add it there, for example:\n\
                 \"oauth\": {{ \"{provider}\": {{ \"clientId\": \"...\", \"authorizeUrl\": \"...\", \
                 \"tokenUrl\": \"...\", \"callbackPort\": 54545, \"scopes\": [\"...\"] }} }}\n\
                 Project settings are not used for login. See CONFIGURATION.md."
            )
        })
}

fn describe_expiry(expires_at: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    if expires_at <= now {
        "expired, refreshes on next use".to_string()
    } else {
        let minutes = (expires_at - now) / 60;
        format!("expires in {minutes} min")
    }
}

/// Waits for the browser redirect on the loopback listener and returns its OAuth params.
///
/// Connections for other paths (favicon requests, browser preconnects that send
/// nothing) are answered or dropped and the wait continues, so only the callback ends
/// it. Times out so an abandoned login does not hang the CLI forever.
fn wait_for_callback(listener: &TcpListener) -> Result<OAuthCallbackParams, String> {
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + CALLBACK_TIMEOUT;
    loop {
        let mut stream = accept_before(listener, deadline)?;
        stream
            .set_nonblocking(false)
            .map_err(|error| error.to_string())?;
        // A silent connection must not stall the wait.
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| error.to_string())?;

        let target = read_request_target(&mut stream);
        if target.split('?').next() != Some("/callback") {
            write_html_response(&mut stream, "404 Not Found", "Not found.");
            continue;
        }

        let parsed = parse_oauth_callback_request_target(&target);
        match &parsed {
            Ok(_) => write_html_response(
                &mut stream,
                "200 OK",
                "Signed in. You can close this tab and return to Frontal Code.",
            ),
            Err(_) => write_html_response(
                &mut stream,
                "400 Bad Request",
                "Login failed. Return to Frontal Code for details.",
            ),
        }
        return parsed.map_err(|error| format!("login failed: {error}"));
    }
}

/// Accepts the next connection, polling until `deadline`.
fn accept_before(listener: &TcpListener, deadline: Instant) -> Result<TcpStream, String> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("login timed out waiting for the browser redirect".to_string());
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// Reads the request line and returns its target (`/path?query`). Empty when the
/// connection sent nothing usable.
fn read_request_target(stream: &mut TcpStream) -> String {
    let mut buffer = [0_u8; 8192];
    let read = stream.read(&mut buffer).unwrap_or(0);
    String::from_utf8_lossy(&buffer[..read])
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("")
        .to_string()
}

/// Best effort: the browser may already have closed the connection.
fn write_html_response(stream: &mut TcpStream, status: &str, message: &str) {
    let body = format!("<html><body><p>{message}</p></body></html>");
    let response = format!(
        "HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

/// Reads the redirect URL the user pastes back, for providers that use a manual redirect.
fn read_pasted_callback() -> Result<OAuthCallbackParams, String> {
    println!("After signing in, paste the full URL you were redirected to and press Enter:");
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    let query = line.trim().split_once('?').map_or("", |(_, query)| query);
    parse_oauth_callback_query(query).map_err(|error| format!("login failed: {error}"))
}

fn open_in_browser(url: &str) -> io::Result<()> {
    let status = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()?
    } else if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()?
    } else {
        Command::new("xdg-open").arg(url).status()?
    };
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "browser command exited with {status}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::thread;

    use super::{validate_provider, wait_for_callback};

    /// Plays the browser: sends one GET to the listener and returns the full response.
    fn send_request(addr: SocketAddr, target: &str) -> String {
        let mut stream = TcpStream::connect(addr).expect("connect to callback listener");
        let request = format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        stream.write_all(request.as_bytes()).expect("write request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read response");
        response
    }

    /// Sends the redirect from a separate thread, so the test can run the listener.
    fn fake_browser_redirect(addr: SocketAddr, target: &'static str) -> thread::JoinHandle<String> {
        thread::spawn(move || send_request(addr, target))
    }

    #[test]
    fn validate_provider_accepts_supported_and_rejects_unknown() {
        assert!(validate_provider("anthropic").is_ok());
        assert!(validate_provider("openai").is_ok());
        assert!(validate_provider("gemini").is_err());
    }

    #[test]
    fn callback_listener_returns_code_and_state_from_redirect() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");
        let browser = fake_browser_redirect(addr, "/callback?code=auth-code-1&state=state-1");

        let params = wait_for_callback(&listener).expect("callback params");
        assert_eq!(params.code.as_deref(), Some("auth-code-1"));
        assert_eq!(params.state.as_deref(), Some("state-1"));

        let response = browser.join().expect("browser thread");
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    }

    #[test]
    fn callback_listener_ignores_other_paths_until_the_callback() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");
        // One browser thread, so the favicon request is fully answered before the
        // callback is sent; the order is deterministic.
        let browser = thread::spawn(move || {
            let favicon = send_request(addr, "/favicon.ico");
            let callback = send_request(addr, "/callback?code=auth-code-2&state=state-2");
            (favicon, callback)
        });

        let params = wait_for_callback(&listener).expect("callback params");
        assert_eq!(params.code.as_deref(), Some("auth-code-2"));
        assert_eq!(params.state.as_deref(), Some("state-2"));

        let (favicon, callback) = browser.join().expect("browser thread");
        assert!(favicon.starts_with("HTTP/1.1 404"), "{favicon}");
        assert!(callback.starts_with("HTTP/1.1 200 OK"), "{callback}");
    }
}
