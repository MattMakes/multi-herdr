//! How panes learn about each other and talk: the worker brief, the mailbox
//! registry, what a message says and how it is typed into a pane.

pub mod brief;
pub mod channel;
pub mod delivery;
pub mod mail;
pub mod mailbox;
pub mod message;
mod private;

/// A name as 1 file or directory name: ASCII letters, digits and `-` stay,
/// every other character (`/`, `:`, `.`, `_`, a space) becomes `_`.
///
/// `.` is replaced on purpose, not kept: a workspace named `..` must not
/// reach a parent directory when it becomes a mail or message directory.
/// Channel sockets use the same rule, so every name horch derives from a
/// workspace, role or pane id is mapped 1 way.
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
