/// A prior finding surfaced as related to some new text, with its similarity.
pub struct Related {
    pub id: i64,
    pub text: String,
    pub cosine: f32,
}
