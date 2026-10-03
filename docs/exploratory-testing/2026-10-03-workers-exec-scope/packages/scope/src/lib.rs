pub mod gen;
pub mod other;

pub fn base(x: i32) -> bool {
    x > 1
}

// mutator-disable-func
pub fn off_all(x: i32) -> bool {
    x > 2
}

// mutator-disable-func conditional/negated
/// Doc comment.
#[inline]
pub fn off_one(x: i32) -> bool {
    x > 3
}

pub fn next_line(x: i32) -> bool {
    // mutator-disable-next-line conditional/negated
    x > 4
}

// mutator-disable-regexp LEGACY
pub fn legacy(x: i32) -> bool {
    x > 5 // LEGACY
}

#[cfg(feature = "fast")]
pub fn gated(x: i32) -> bool {
    x > 6
}

pub struct S;
impl S {
    // mutator-disable-func
    pub fn method_off(&self, x: i32) -> bool {
        x > 7
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn t() { assert!(super::base(5)); }
}
