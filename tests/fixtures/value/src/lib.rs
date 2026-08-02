#[derive(Debug, Default, PartialEq)]
pub struct Config {
    pub count: i32,
    pub enabled: bool,
    pub label: &'static str,
    pub context: Option<i32>,
}

pub fn config() -> Config {
    Config {
        count: 3,
        enabled: true,
        label: "value",
        context: Some(2),
        ..Default::default()
    }
}

pub fn self_assignment(mut value: i32) -> i32 {
    value = value;
    value
}

fn unwrap_context(value: Option<i32>) -> i32 {
    value.unwrap_or(-1)
}

pub fn with_context(value: i32) -> i32 {
    unwrap_context(Some(value))
}

fn generic_context<T>(_: Option<T>) {}

pub fn inference_sensitive() {
    generic_context(Some(1));
}

pub fn enabled() -> bool {
    return true;
}

pub fn generic<T: Default>(value: T) -> T {
    return value;
}

pub fn borrowed<'a>(value: &'a [i32]) -> &'a [i32] {
    return value;
}

pub fn owned(owned_value: Vec<i32>) -> Vec<i32> {
    return owned_value;
}

pub fn unsupported_result(value: i32) -> Result<i32, &'static str> {
    return Ok(value);
}

pub fn unsupported_generic<T>(value: T) -> T {
    return value;
}

pub fn unsupported_borrow<T>(value: &T) -> &T {
    return value;
}
