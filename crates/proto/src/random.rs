//! Secrets from the operating system's generator (`getrandom`): the engine tokens and the
//! same-host nonces. `/dev/urandom` is Unix-only; this works on Windows too.

/// `bytes` random bytes as lowercase hex (`2 * bytes` digits).
pub fn hex(bytes: usize) -> std::io::Result<String> {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).map_err(|e| std::io::Error::other(format!("no random bytes: {e}")))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn hex_is_the_asked_length_and_differs_each_time() {
        let a = super::hex(32).unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_ne!(a, super::hex(32).unwrap());
    }
}
