use crate::infrastructure::db::Annotation;

pub fn annotation_target(a: &Annotation) -> String {
    match (&a.target_kind, &a.target_id) {
        (Some(k), Some(idv)) => format!(" [{k}:{idv}]"),
        _ => String::new(),
    }
}
