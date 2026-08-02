#[test]
fn detects_control_flow_changes() {
    assert_eq!(control_flow_mutator_fixture::branches(true), 1);
    assert_eq!(control_flow_mutator_fixture::branches(false), 2);
    assert_eq!(control_flow_mutator_fixture::choice(0), 3);
    assert_eq!(control_flow_mutator_fixture::first(&[1, 2]), 1);
    assert_eq!(control_flow_mutator_fixture::first_nonzero(&[0, 4]), 4);
    assert_eq!(control_flow_mutator_fixture::countdown(3), 0);

    let mut value = 0;
    control_flow_mutator_fixture::set(&mut value);
    assert_eq!(value, 3);

    let mut values = Vec::new();
    control_flow_mutator_fixture::record(&mut values);
    assert_eq!(values, [1, 2]);

    let mut conditional = Vec::new();
    control_flow_mutator_fixture::record_if(true, &mut conditional);
    control_flow_mutator_fixture::record_if(false, &mut conditional);
    assert_eq!(conditional, [3]);
}
