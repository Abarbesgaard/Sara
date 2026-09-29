pub(super) fn print_cleared(id: i64) {
    println!("Removed branch tie from task {id}.");
}

pub(super) fn print_tied(id: i64, branch: &str) {
    println!("Tied task {id} to branch '{branch}'.");
    println!("Run `sara stop {id}` to snapshot changed files.");
}
