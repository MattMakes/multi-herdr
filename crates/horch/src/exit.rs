pub const SUCCESS: u8 = 0;
pub const FAILURE: u8 = 1;
pub const COLLECTOR_HELD: u8 = 2;
pub const REFUSED: u8 = 3;
pub const COMPACTION_REPORTED: u8 = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_special_exit_code_has_1_meaning() {
        let codes = [FAILURE, COLLECTOR_HELD, REFUSED, COMPACTION_REPORTED];
        let distinct: std::collections::BTreeSet<u8> = codes.into_iter().collect();
        assert_eq!(distinct.len(), codes.len());
        assert!(!distinct.contains(&SUCCESS));
    }
}
