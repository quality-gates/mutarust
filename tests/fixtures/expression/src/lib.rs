pub fn base(a: i32, b: i32) -> i32 {
    a + b
}

pub fn bitwise(a: u8, b: u8) -> u8 {
    a & b
}

pub fn assignment(mut a: i32, b: i32) -> i32 {
    a += b;
    a
}

pub fn negate(a: i32) -> i32 {
    -a
}

pub fn number() -> i32 {
    let value = 2;
    value
}

pub fn float() -> f64 {
    let value = 2.5;
    value
}

pub fn boolean() -> bool {
    let value = true;
    value
}

pub fn comparison(a: i32, b: i32) -> bool {
    a < b
}

pub fn not(a: bool) -> bool {
    if !a { true } else { false }
}

pub fn logical(a: bool, b: bool) -> bool {
    a && b
}

pub fn string(value: &str) -> bool {
    value == "yes"
}

