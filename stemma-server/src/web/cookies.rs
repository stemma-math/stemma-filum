//! The two cookies the instance sets.
//!
//! Both are written by hand rather than through a library: their values are
//! hexadecimal and identifiers, so there is nothing to escape, and the
//! attributes that matter — `HttpOnly`, `SameSite`, `Secure` — are then
//! visible in one place.

use axum::http::HeaderMap;
use axum::http::header::{COOKIE, HeaderValue, SET_COOKIE};

/// The cookie that says whose session this is.
pub const SESSION: &str = "stemma_session";

/// Reads one cookie out of a request.
pub fn read<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|header| header.to_str().ok())
        .flat_map(|header| header.split(';'))
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| key.trim() == name)
        .map(|(_, value)| value.trim())
}

/// A cookie that lasts, for a session the server will expire on its own.
pub fn set(name: &str, value: &str, seconds: i64, secure: bool) -> HeaderValue {
    build(name, value, &format!("Max-Age={seconds}"), secure)
}

/// A cookie that tells the browser to forget what it was holding.
pub fn clear(name: &str, secure: bool) -> HeaderValue {
    build(name, "", "Max-Age=0", secure)
}

/// Assembles the header.
fn build(name: &str, value: &str, lifetime: &str, secure: bool) -> HeaderValue {
    let secure = match secure {
        true => "; Secure",
        false => "",
    };

    HeaderValue::from_str(&format!(
        "{name}={value}; Path=/; HttpOnly; SameSite=Lax; {lifetime}{secure}"
    ))
    .expect("the parts of a cookie are all printable")
}

/// Puts a cookie on a response.
pub fn attach(headers: &mut HeaderMap, cookie: HeaderValue) {
    headers.append(SET_COOKIE, cookie);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(cookie: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, HeaderValue::from_str(cookie).unwrap());

        headers
    }

    #[test]
    fn one_cookie_is_found_among_several() {
        let headers = headers("other=1; stemma_session=ses:01.abc; another=2");

        assert_eq!(read(&headers, SESSION), Some("ses:01.abc"));
        assert_eq!(read(&headers, "nothing"), None);
    }

    #[test]
    fn a_request_with_no_cookies_has_none() {
        assert_eq!(read(&HeaderMap::new(), SESSION), None);
    }

    #[test]
    fn a_session_cookie_is_not_readable_from_a_script() {
        let cookie = set(SESSION, "value", 60, true);
        let written = cookie.to_str().unwrap();

        assert!(written.contains("HttpOnly"));
        assert!(written.contains("SameSite=Lax"));
        assert!(written.contains("Secure"));
        assert!(written.contains("Max-Age=60"));
    }

    #[test]
    fn an_instance_without_encryption_does_not_claim_it_has_it() {
        let cookie = set(SESSION, "value", 60, false);

        assert!(!cookie.to_str().unwrap().contains("Secure"));
    }

    #[test]
    fn clearing_a_cookie_empties_it_and_expires_it_at_once() {
        let cookie = clear(SESSION, false);
        let written = cookie.to_str().unwrap();

        assert!(written.starts_with("stemma_session=;"));
        assert!(written.contains("Max-Age=0"));
    }
}
