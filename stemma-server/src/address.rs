//! Email addresses, checked as far as is honest and no further.
//!
//! Whether an address exists can only be learned by writing to it, and
//! this instance may have no way to write to anything. So the check is
//! for shape alone — and it is the same check wherever an address is
//! typed, whether into the registration form or onto the machine's own
//! command line.

/// Whether a string looks like somewhere mail could go.
pub fn check(email: &str) -> Result<(), String> {
    match email.split_once('@') {
        Some((before, after))
            if !before.is_empty()
                && after.contains('.')
                && !after.starts_with('.')
                && !after.ends_with('.')
                && !email.contains(char::is_whitespace) =>
        {
            Ok(())
        }
        _ => Err(format!("`{email}` does not look like an email address.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_without_a_host_is_refused() {
        for wrong in [
            "ana",
            "ana@",
            "@example.org",
            "ana@example",
            "a b@example.org",
        ] {
            assert!(check(wrong).is_err(), "took `{wrong}`");
        }
    }

    #[test]
    fn an_ordinary_address_is_accepted() {
        for right in ["ana@example.org", "ana.ferrer+stemma@uv.es"] {
            assert!(check(right).is_ok(), "refused `{right}`");
        }
    }
}
