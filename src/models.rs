#[derive(Debug, Clone)]
pub struct Topic {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub id: i64,
    pub text: String,
    pub url: Option<String>,
    pub created_at: String,   // RFC3339
}

#[derive(Debug, Clone)]
pub struct Todo {
    pub id: i64,
    pub topic_id: i64,
    pub text: String,
    pub done: bool,
    pub url: Option<String>,
    pub due_date: Option<String>,    // ISO date "YYYY-MM-DD"
    pub priority: Option<u8>,        // 1 = high, 2 = medium, 3 = low
    pub in_progress: bool,
}
