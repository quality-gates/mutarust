pub fn a(x: u32) -> bool { x > 10 }
pub fn b(x: u32) -> bool { x < 20 }

#[cfg(test)]
mod tests {
    #[test]
    fn slow() {
        std::thread::sleep(std::time::Duration::from_secs(4));
        assert!(super::a(11));
        assert!(super::b(19));
    }
}
