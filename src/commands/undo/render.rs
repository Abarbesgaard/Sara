/// Report the undone command, or that the history was empty.
pub(super) fn print_undo(command: Option<&str>) {
    match command {
        Some(command) => println!("Undid: {command}"),
        None => println!("Nothing to undo."),
    }
}
