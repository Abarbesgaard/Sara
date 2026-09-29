use std::path::Path;

use anyhow::{Context, Result};

pub(super) fn emit(output: Option<&Path>, blob: &str, root_id: i64, extra: usize) -> Result<()> {
    match output {
        Some(path) => {
            std::fs::write(path, format!("{blob}\n"))
                .with_context(|| format!("writing blob to {}", path.display()))?;
            let dep_note = if extra > 0 {
                format!(
                    " (+{extra} dependency task{})",
                    if extra == 1 { "" } else { "s" }
                )
            } else {
                String::new()
            };
            eprintln!("Exported task {root_id}{dep_note} to {}", path.display());
        }
        None => {
            println!("{blob}");
            if extra > 0 {
                eprintln!(
                    "Exported task {root_id} with {extra} dependency task{}. Import with `sara import`.",
                    if extra == 1 { "" } else { "s" }
                );
            } else {
                eprintln!("Exported task {root_id}. Import with `sara import`.");
            }
        }
    }
    Ok(())
}
