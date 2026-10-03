//! Promotion (phase B5): dataset path safety, the git promotion engine,
//! restart, rollback and round cleanup.

use std::path::Path;

use horch_core::ids::{ExperimentId, RoundId};
use horch_core::measure::paths::DatasetPaths;

// ─── dataset paths ──────────────────────────────────────────────────────────

#[test]
fn dataset_paths_reject_traversal_ids() {
    let paths = DatasetPaths::from_slug(Path::new("/s"), "p");
    let good_exp = ExperimentId::new("e").unwrap();
    let good_round = RoundId::new("r").unwrap();
    // The id types keep accepting legacy ids; the paths refuse them.
    for bad in ["../x", "a/b", ".", "..", "a\\b", "x/../y"] {
        let exp = ExperimentId::new(bad).unwrap();
        let round = RoundId::new(bad).unwrap();
        assert!(paths.experiment_dir(&exp).is_err(), "{bad:?}");
        assert!(paths.manifest(&exp).is_err(), "{bad:?}");
        assert!(paths.rounds_dir(&exp).is_err(), "{bad:?}");
        assert!(paths.default_worktree_root(&exp).is_err(), "{bad:?}");
        assert!(paths.round_file(&good_exp, &round).is_err(), "{bad:?}");
        assert!(paths.round_file(&exp, &good_round).is_err(), "{bad:?}");
        assert!(paths.artifacts_dir(&good_exp, &round).is_err(), "{bad:?}");
        assert!(paths.judge_input_dir(&good_exp, &round).is_err(), "{bad:?}");
        assert!(paths.judgement(&round).is_err(), "{bad:?}");
        assert!(paths.promotion(&round).is_err(), "{bad:?}");
        assert!(paths.jobs_dir(&round).is_err(), "{bad:?}");
        assert!(paths.job_dir(&round, 1).is_err(), "{bad:?}");
        assert!(paths.exports_dir(bad).is_err(), "{bad:?}");
        assert!(
            paths.validation_dir(&good_exp, &good_round, bad).is_err(),
            "{bad:?}"
        );
        let err = paths.promotion(&round).unwrap_err().to_string();
        assert!(err.contains("round id"), "{err}");
    }
    // Empty: the id types refuse it, so only the string accessors see it.
    assert!(paths.exports_dir("").is_err());
    assert!(paths.validation_dir(&good_exp, &good_round, "").is_err());
    assert!(paths.exports_dir("a\0b").is_err());
    // Plain ids still work, dots inside a name too.
    let ok = RoundId::new("r.1").unwrap();
    assert_eq!(
        paths.promotion(&ok).unwrap(),
        Path::new("/s/multi-herdr/p/promotions/r.1.json")
    );
}
