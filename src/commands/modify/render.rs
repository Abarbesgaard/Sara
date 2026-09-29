/// Confirm a task was updated.
pub(super) fn print_updated(id: i64, description: &str) {
    println!("Updated task {id}: {description}");
}

pub(super) fn print_cancelled() {
    println!("Cancelled.");
}
