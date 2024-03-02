use std::{
    collections::HashMap,
    convert::Infallible,
    future::{ready, Ready},
};

use axum::{
    extract::{MatchedPath, Query, Request},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use futures::future::BoxFuture;
use tokio::net::TcpListener;
use tower::Service;
use tower_fallthrough_filter::{AsyncFilter, AsyncFilterLayer};

// Imagine that this middleware could read files from
// the disk and render the html using templating engines.
#[derive(Clone)]
struct MyMiddleware;

impl Service<Request> for MyMiddleware {
    type Response = Response;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let query = Query::<HashMap<String, String>>::try_from_uri(req.uri())
            .expect("Could not extract query from the request!");

        let name = match query.get("name") {
            Some(name) => name,
            None => "Unknown",
        };

        ready(Ok(Html(format!("Hello {name}!")).into_response()))
    }
}

#[derive(Clone)]
struct MatchesRouteFilter;

impl AsyncFilter<Request> for MatchesRouteFilter {
    type Future = BoxFuture<'static, bool>;

    fn matches(&self, req: &Request) -> Self::Future {
        // Execute the layer only if the "matched path"
        // is none. Meaning that the paths isn't registered
        // to a handler already.

        let matches = req.extensions().get::<MatchedPath>().is_none();

        Box::pin(async move { matches })

        // Imagine that the middleware is a bit more complex
        // and renders html files from a directory. If we want
        // to render only for paths that exist in the directory
        // we can share something that lets the filter know
        // if a path is known and compare `req.uri().path()`
        // to it.
    }
}

#[tokio::main]
async fn main() {
    // If we directly register the layer using `app.layer(service)`
    // it will also handle requests for already defined routes like
    // `/api/hello`.
    let service = MyMiddleware;
    let filter = MatchesRouteFilter;

    let layer = AsyncFilterLayer::new(filter, service);

    let app = Router::<()>::new()
        .nest(
            "/api",
            Router::new().route("/hello", get(move || async { "Hello, World!" })),
        )
        .layer(layer);

    let listener = TcpListener::bind("127.0.0.1:1337")
        .await
        .expect("Failed to create TCP Listener!");

    println!("Listening on http://127.0.0.1:1337/");
    println!();
    println!("Try to open: http://127.0.0.1:1337/unknown?name=Rust");
    println!("Try to open: http://127.0.0.1:1337/api/hello");

    axum::serve(listener, app)
        .await
        .expect("Failed to start axum server!")
}
