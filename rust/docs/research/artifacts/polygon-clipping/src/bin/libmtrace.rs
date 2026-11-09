fn main() {
    let path = std::env::args().nth(1).unwrap();
    std::fs::write(path, clipresearch::libmtrace::trace_text()).unwrap();
}
