#[test]
fn detects_standard_thread_mutations() {
    let results = concurrency_selection_mutator_fixture::standard_spawns_use_new_threads();
    assert_eq!(&results[..2], &[false; 2]);
}

#[tokio::test(flavor = "current_thread")]
async fn detects_task_and_selection_mutations() {
    assert_eq!(
        concurrency_selection_mutator_fixture::task_spawns_use_new_tasks().await,
        [false; 4]
    );
    assert_eq!(
        concurrency_selection_mutator_fixture::select_value(0).await,
        "outer-fallback"
    );
    assert_eq!(
        concurrency_selection_mutator_fixture::select_value(2).await,
        "inner-first"
    );
    assert_eq!(
        concurrency_selection_mutator_fixture::select_value(3).await,
        "inner-second"
    );
    assert_eq!(
        concurrency_selection_mutator_fixture::select_value(4).await,
        "inner-fallback"
    );
}
