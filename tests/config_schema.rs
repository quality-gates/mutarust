use serde_json::Value;

#[test]
fn configuration_schema_is_strict_and_describes_every_supported_field() {
    let schema: Value = serde_json::from_str(include_str!("../schema/mutarust.schema.json"))
        .expect("configuration schema must be valid JSON");
    let object = schema
        .as_object()
        .expect("configuration schema must be a JSON object");

    assert_eq!(
        object.get("additionalProperties"),
        Some(&Value::Bool(false)),
        "the configuration schema must reject unknown fields"
    );
    let properties = object
        .get("properties")
        .and_then(Value::as_object)
        .expect("configuration schema must describe properties");
    let expected = [
        "skip_without_test",
        "skip_with_cfg",
        "json_output",
        "html_output",
        "silent_mode",
        "min_msi",
        "min_covered_msi",
        "exclude_dirs",
        "disable_mutators",
        "enable_mutators",
        "ignore_source_lines",
    ];
    assert_eq!(
        properties.len(),
        expected.len(),
        "the configuration schema must have no undocumented fields"
    );
    for field in expected {
        assert!(
            properties.contains_key(field),
            "the configuration schema must describe {field}"
        );
    }

    for field in ["min_msi", "min_covered_msi"] {
        let score = properties
            .get(field)
            .and_then(Value::as_object)
            .expect("score field must be an object");
        assert_eq!(score.get("minimum"), Some(&Value::from(0)));
        assert_eq!(score.get("maximum"), Some(&Value::from(100)));
    }
}
