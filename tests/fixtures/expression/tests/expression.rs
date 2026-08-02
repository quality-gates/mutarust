#[test]
fn detects_expression_changes() {
    assert_eq!(expression_mutator_fixture::base(7, 2), 9);
    assert_eq!(expression_mutator_fixture::bitwise(6, 3), 2);
    assert_eq!(expression_mutator_fixture::assignment(7, 2), 9);
    assert_eq!(expression_mutator_fixture::negate(3), -3);
    assert_eq!(expression_mutator_fixture::number(), 2);
    assert_eq!(expression_mutator_fixture::float(), 2.5);
    assert!(expression_mutator_fixture::boolean());
    assert!(expression_mutator_fixture::comparison(1, 2));
    assert!(expression_mutator_fixture::not(false));
    assert!(!expression_mutator_fixture::logical(true, false));
    assert!(expression_mutator_fixture::string("yes"));
}

