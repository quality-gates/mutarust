use value_mutator_fixture::Config;

#[test]
fn detects_value_mutations() {
    assert_eq!(
        value_mutator_fixture::config(),
        Config {
            count: 3,
            enabled: true,
            label: "value",
            context: Some(2),
        }
    );
    assert_eq!(value_mutator_fixture::self_assignment(7), 7);
    assert_eq!(value_mutator_fixture::with_context(5), 5);
    value_mutator_fixture::inference_sensitive();
    assert!(value_mutator_fixture::enabled());
    assert_eq!(value_mutator_fixture::generic(7), 7);
    assert_eq!(value_mutator_fixture::borrowed(&[1, 2]), &[1, 2]);
    assert_eq!(value_mutator_fixture::owned(vec![1, 2]), vec![1, 2]);
    assert_eq!(value_mutator_fixture::unsupported_result(9), Ok(9));
    assert_eq!(value_mutator_fixture::unsupported_generic(8), 8);
    assert_eq!(value_mutator_fixture::unsupported_borrow(&6), &6);
}
