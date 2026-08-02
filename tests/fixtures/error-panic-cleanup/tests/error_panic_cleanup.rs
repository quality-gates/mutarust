use std::cell::Cell;
use std::error::Error;

#[test]
fn detects_error_source_mutations() {
    assert!(
        error_panic_cleanup_mutator_fixture::ObservedWrapper::new()
            .source()
            .is_some()
    );
}

#[test]
fn detects_panic_and_cleanup_mutations() {
    assert!(error_panic_cleanup_mutator_fixture::observed_recovery());
    let cell = Cell::new(0);
    assert_eq!(
        error_panic_cleanup_mutator_fixture::observed_cleanup(&cell),
        1
    );
    assert_eq!(
        error_panic_cleanup_mutator_fixture::lifetime_checked_cleanup(),
        2
    );
}
