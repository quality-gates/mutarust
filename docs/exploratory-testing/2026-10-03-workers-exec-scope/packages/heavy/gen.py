# Writes heavy/src/lib.rs: two small target functions and 3000 generic functions that take long to compile.
lines = ["pub fn target(x: u32) -> bool { x > 10 && x < 99 }", "pub fn target2(x: u32) -> u32 { x + 3 }"]
for i in range(1500):
    lines.append(f"pub fn f{i}<T: Clone + std::fmt::Debug>(v: Vec<T>) -> Vec<String> {{ v.iter().map(|x| format!(\"{{:?}}{i}\", x)).collect() }}")
    lines.append(f"pub fn g{i}() -> usize {{ f{i}(vec![1u8, 2, 3]).len() + f{i}(vec![\"a\"]).len() }}")
lines.append("#[cfg(test)]\nmod tests {\n #[test]\n fn t() { assert!(super::target(11)); assert_eq!(super::target2(1), 4); }\n}")
open("src/lib.rs", "w").write("\n".join(lines) + "\n")
