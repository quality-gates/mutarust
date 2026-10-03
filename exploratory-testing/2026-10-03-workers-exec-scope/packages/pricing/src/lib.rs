pub fn discount(total: u32, member: bool) -> u32 {
    if member && total > 100 {
        total - 10
    } else {
        total
    }
}

pub fn is_free(total: u32) -> bool {
    total == 0
}

pub fn tax(total: u32) -> u32 {
    total * 20 / 100
}

pub fn sum(items: &[u32]) -> u32 {
    let mut acc = 0;
    for i in items {
        acc += i;
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn member_discount() {
        assert_eq!(discount(200, true), 190);
    }
    #[test]
    fn free() {
        assert!(is_free(0));
    }
    #[test]
    fn taxed() {
        assert_eq!(tax(100), 20);
    }
}
