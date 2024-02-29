use std::{
    marker::PhantomData,
    task::{Context, Poll},
};

use futures::future::BoxFuture;
use tower::{Layer, Service};

pub trait Filter<T> {
    /// Whether the filtered_layer should be executed
    fn matches(&self, item: &T) -> bool;
}

#[derive(Debug)]
pub struct FilterLayer<F, S, R, E, T> {
    filter: F,
    filtered_service: S,

    _marker: PhantomData<(R, E, T)>,
}

impl<F: Clone, S: Clone, R, E, T> Clone for FilterLayer<F, S, R, E, T> {
    fn clone(&self) -> Self {
        Self {
            filter: self.filter.clone(),
            filtered_service: self.filtered_service.clone(),

            _marker: PhantomData,
        }
    }
}

impl<T, S: Service<T>, F: Filter<T>> FilterLayer<F, S, S::Response, S::Error, T> {
    pub fn new(filter: F, filtered_service: S) -> Self {
        Self {
            filter,
            filtered_service,

            _marker: PhantomData,
        }
    }
}

impl<F, S1, S2, R, E, T> Layer<S2> for FilterLayer<F, S1, R, E, T>
where
    F: Clone,
    S1: Clone,
{
    type Service = FilterService<F, S1, S2, R, E>;

    fn layer(&self, inner_service: S2) -> Self::Service {
        let filter = self.filter.clone();
        let filtered_service = self.filtered_service.clone();

        FilterService {
            filter,
            filtered_service,
            inner_service,

            _marker: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct FilterService<F, Filtered, Inner, Response, Error> {
    filter: F,
    filtered_service: Filtered,
    inner_service: Inner,

    _marker: PhantomData<(Response, Error)>,
}

impl<F: Clone, Filtered: Clone, Inner: Clone, Response, Error> Clone
    for FilterService<F, Filtered, Inner, Response, Error>
{
    fn clone(&self) -> Self {
        Self {
            filter: self.filter.clone(),
            filtered_service: self.filtered_service.clone(),
            inner_service: self.inner_service.clone(),

            _marker: PhantomData,
        }
    }
}

impl<F, Filtered, Inner, Request, Response, Error> Service<Request>
    for FilterService<F, Filtered, Inner, Response, Error>
where
    F: Filter<Request>,
    Filtered: Service<Request, Response = Response, Error = Error>,
    Filtered::Future: Send + 'static,
    Inner: Service<Request, Response = Response, Error = Error>,
    Inner::Future: Send + 'static,
{
    type Response = Filtered::Response;
    type Error = Filtered::Error;
    // TODO: It should be possible to replace this by a custom Future
    type Future = BoxFuture<'static, Result<Response, Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.filtered_service.poll_ready(cx)

        // NOTE: Should I be poll_ready the inner service as well?
    }

    fn call(&mut self, req: Request) -> Self::Future {
        if self.filter.matches(&req) {
            let fut = self.filtered_service.call(req);

            Box::pin(async move { fut.await })
        } else {
            let fut = self.inner_service.call(req);

            Box::pin(async move { fut.await })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        task::{Context, Poll},
    };

    use axum::{extract::Request, response::Response, routing::get, Router};
    use axum_test::TestServer;
    use futures::future::{ready, Ready};
    use tower::Service;

    use super::*;

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
    struct TestFilter(bool);

    impl<T> Filter<T> for TestFilter {
        fn matches(&self, _: &T) -> bool {
            self.0
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
    async fn should_allow() {
        let service_a = TestService("a".into());

        let filter = TestFilter(true);
        let filter_layer = FilterLayer::new(filter, service_a);

        let app = Router::<()>::new()
            .route("/test", get(move || async { "b" }))
            .layer(filter_layer);

        let server = TestServer::new(app).unwrap();

        let res = server.get("/test").await;
        let text = res.text();

        assert_eq!(text, "a");
    }

    #[tokio::test]
    async fn should_fall_through() {
        let service_a = TestService("a".into());

        let filter = TestFilter(false);
        let filter_layer = FilterLayer::new(filter, service_a);

        let app = Router::<()>::new()
            .route("/test", get(move || async { "b" }))
            .layer(filter_layer);

        let server = TestServer::new(app).unwrap();

        let res = server.get("/test").await;
        let text = res.text();

        assert_eq!(text, "b");
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
}
