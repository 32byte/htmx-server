use std::{
    convert::Infallible,
    task::{Context, Poll},
};

use axum::{extract::Request, response::Response, routing::get, Router};
use axum_test::TestServer;
use futures::future::{ready, Ready};
use tower::Service;

use tower_fallthrough_filter::*;

#[derive(Debug)]
struct TestService(String);

impl Clone for TestService {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Service<Request> for TestService {
    type Response = Response;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: Request) -> Self::Future {
        ready(Ok(Response::new(self.0.clone().into())))
    }
}

#[derive(Debug, Clone)]
struct DynamicFilter;

impl Filter<Request> for DynamicFilter {
    fn matches(&self, req: &Request) -> bool {
        req.uri().path().contains("filter")
    }
}

#[tokio::test]
async fn should_dynamically_filter() {
    let service_a = TestService("a".into());

    let filter = DynamicFilter;
    let filter_layer = FilterLayer::new(filter, service_a);

    let app = Router::<()>::new()
        .route("/test", get(move || async { "b" }))
        .route("/filter", get(move || async { "c" }))
        .route("/123-filter-asdf", get(move || async { "d" }))
        .layer(filter_layer);

    let server = TestServer::new(app).unwrap();

    let res = server.get("/test").await;
    let text = res.text();

    assert_eq!(text, "b");

    let res = server.get("/filter").await;
    let text = res.text();

    assert_eq!(text, "a");

    let res = server.get("/123-filter-asdf").await;
    let text = res.text();

    assert_eq!(text, "a");
}