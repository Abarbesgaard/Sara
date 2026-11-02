use crate::domain::value_objects::Relation;

#[derive(Debug, Clone)]
pub struct MemoryLink {
    pub id: i64,
    pub from_uuid: String,
    pub to_uuid: String,
    pub relation: Relation,
    pub weight: f64,
}
