pub fn prompt_line(msg: &str) -> anyhow::Result<String> {
    use std::io::Write;
    print!("{msg}");
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

pub fn print_cancelled() {
    println!("Cancelled.");
}
