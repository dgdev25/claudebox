pub fn info(message: &str) {
    eprintln!("{message}");
}

pub fn warn(message: &str) {
    eprintln!("Warning: {message}");
}

pub fn action(message: &str) {
    eprintln!("{message}...");
}
