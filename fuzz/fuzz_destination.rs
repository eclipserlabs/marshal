use marshall::{destination::host_of, validate_destination};

fn main() {
    // libFuzzer entry: cargo fuzz run fuzz_destination
    // For `cargo run --bin fuzz_destination` we run a simple corpus.
    // `cargo run --bin fuzz_destination -- <url>` checks a single URL.
    let inputs: Vec<String> = std::env::args().skip(1).filter(|a| a != "--").collect();
    if let Some(input) = inputs.first() {
        // Historical README used `-- 10` as a count; treat a bare number as
        // "run corpus" for backwards compat instead of silently checking "10".
        if input.parse::<usize>().is_ok() {
            run_corpus();
            return;
        }
        let _ = host_of(input);
        let _ = validate_destination(input);
        println!("checked: {input}");
        return;
    }
    run_corpus();
}

fn run_corpus() {
    // Simple inline corpus smoke
    let corpus = [
        "https://example.com/",
        "https://169.254.169.254/",
        "https://[::ffff:169.254.169.254]/",
        "https://example.com@169.254.169.254/",
        "https://example.com/\r\nHost: evil",
        "https://example.com:22/",
        "file:///etc/passwd",
        "http://127.0.0.1:8080/",
        "https://[2002:a9fe:a9fe::1]/",
        "https://[fd00::1]/",
    ];
    for url in corpus {
        let _ = host_of(url);
        let _ = validate_destination(url);
    }
    println!("fuzz corpus ok: {}", corpus.len());
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corpus_does_not_panic() {
        main();
    }
}
