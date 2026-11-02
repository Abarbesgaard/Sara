#[derive(Debug, Clone)]
pub struct Anchor {
    pub path: String,
    pub source: String,
    pub reason: Option<String>,
    pub symbol: Option<String>,
    pub line_start: Option<i64>,
    pub line_end: Option<i64>,
}

impl Anchor {
    pub fn location(&self) -> String {
        let mut s = String::new();
        if let Some(sym) = &self.symbol {
            s.push_str(" :: ");
            s.push_str(sym);
        }
        match (self.line_start, self.line_end) {
            (Some(a), Some(b)) => s.push_str(&format!(" ({a}-{b})")),
            (Some(a), None) => s.push_str(&format!(" (L{a})")),
            _ => {}
        }
        s
    }
}
