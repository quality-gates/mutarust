macro_rules! marker {
    ($target:expr) => {
        $target.push(2)
    };
}

pub fn branches(flag: bool) -> i32 {
    if flag { return 1; } else { return 2; }
    0
}

pub fn choice(value: i32) -> i32 {
    match value { 0 => { return 3; }, _ => () }
    0
}

pub fn first(values: &[i32]) -> usize {
    let mut seen = 0;
    for _ in values { seen += 1; break; }
    seen
}

pub fn first_nonzero(values: &[i32]) -> i32 {
    for value in values { if *value == 0 { continue; } return *value; }
    -1
}

pub fn countdown(value: i32) -> usize {
    let mut values = (0..value).collect::<Vec<_>>();
    while values.pop().is_some() {}
    values.len()
}

pub fn set(value: &mut i32) {
    *value = 3;
}

pub fn record(target: &mut Vec<i32>) {
    target.push(1);
    marker!(target);
}

pub fn record_if(enabled: bool, target: &mut Vec<i32>) {
    if enabled { target.push(3); }
}
