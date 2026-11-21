pub mod brief;
pub mod channel;
pub mod delivery;
pub mod mail;
pub mod mailbox;
pub mod message;
mod private;
pub mod report;

pub(crate) fn file_safe(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::file_safe;

    #[test]
    fn file_safe_keeps_letters_digits_and_dash_only() {
        assert_eq!(file_safe("w4W:p1/a b.c_d-E9"), "w4W_p1_a_b_c_d-E9");
    }

    #[test]
    fn file_safe_never_leaves_a_dot_segment() {
        assert_eq!(file_safe(".."), "__");
        assert_eq!(file_safe("."), "_");
    }
}
