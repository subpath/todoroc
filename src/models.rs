#[derive(Debug, Clone)]
pub struct Topic {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Todo {
    pub id: i64,
    pub topic_id: i64,
    pub text: String,
    pub done: bool,
    pub url: Option<String>,
    pub due_date: Option<String>, // ISO date "YYYY-MM-DD"
}
