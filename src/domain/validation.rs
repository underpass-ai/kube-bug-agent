pub fn nonempty(value: &str) -> bool {
    !value.trim().is_empty()
}
pub fn identifier(value: &str) -> bool {
    nonempty(value)
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-/".contains(&c))
}
pub fn dns_label(value: &str) -> bool {
    value
        .bytes()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}
pub fn dns_name(value: &str) -> bool {
    value
        .split('.')
        .all(|label| label.len() <= 63 && dns_label(label))
}
pub fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}
