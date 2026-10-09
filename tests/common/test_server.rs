use axum::Router;
pub struct TestServer {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl TestServer {
    pub async fn new(router: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { url, task }
    }
    pub fn url(&self) -> &str {
        &self.url
    }
}
impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
