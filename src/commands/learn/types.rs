#[derive(Default)]
pub struct LearnRequest<'a> {
    pub text: &'a str,
    pub tags: &'a [String],
    pub projects: &'a [String],
    pub tasks: &'a [String],
    pub files: &'a [String],
    pub auto_files: bool,
    pub force: bool,
    pub supersedes: &'a [String],
    pub derived_from: &'a [String],
    pub similar_to: &'a [String],
}
