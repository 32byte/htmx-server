use std::{
    convert::Infallible,
    marker::PhantomData,
    task::{Context, Poll},
};

use axum::{extract::Request, response::Response, routing::get, Router};
use axum_test::TestServer;
use futures::{
    future::{ready, BoxFuture, Ready},
    ready, Future,
};
use tower::{Layer, Service};

#[derive(Debug)]
pub struct AsyncFilterLayer<Fun, Fut, Ser, Req, Res, Err>
where
    Fun: Fn(&Req) -> Fut + Clone,
    Fut: Future<Output = bool> + Send + 'static,
    Ser: Service<Req, Response = Res, Error = Err>,
{
    filter: Fun,
    service: Ser,

    _marker: PhantomData<(Fut, Req, Res, Err)>,
}

impl<Fun, Fut, Ser, Req, Res, Err> AsyncFilterLayer<Fun, Fut, Ser, Req, Res, Err>
where
    Fun: Fn(&Req) -> Fut + Clone,
    Fut: Future<Output = bool> + Send + 'static,
    Ser: Service<Req, Response = Res, Error = Err>,
{
    pub fn new(filter: Fun, service: Ser) -> Self {
        Self {
            filter,
            service,

            _marker: PhantomData,
        }
    }
}

impl<Fun, Fut, Ser, Req, Res, Err> Clone for AsyncFilterLayer<Fun, Fut, Ser, Req, Res, Err>
where
    Fun: Fn(&Req) -> Fut + Clone,
    Fut: Future<Output = bool> + Send + 'static,
    Ser: Service<Req, Response = Res, Error = Err> + Clone,
{
    fn clone(&self) -> Self {
        Self {
            filter: self.filter.clone(),
            service: self.service.clone(),

            _marker: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct AsyncFilterService<Fun, Fut, Ser, Inn, Req, Res, Err>
where
    Fun: Fn(&Req) -> Fut,
    Fut: Future<Output = bool> + Send + 'static,
    Ser: Service<Req, Response = Res, Error = Err>,
    Inn: Service<Req, Response = Res, Error = Err>,
{
    filter: Fun,
    service: Ser,
    fallthrough: Inn,

    _marker: PhantomData<(Fut, Req, Res, Err)>,
}

impl<Fun, Fut, Ser, Inn, Req, Res, Err> Clone
    for AsyncFilterService<Fun, Fut, Ser, Inn, Req, Res, Err>
where
    Fun: Fn(&Req) -> Fut + Clone,
    Fut: Future<Output = bool> + Send + 'static,
    Ser: Service<Req, Response = Res, Error = Err> + Clone,
    Inn: Service<Req, Response = Res, Error = Err> + Clone,
{
    fn clone(&self) -> Self {
        Self {
            filter: self.filter.clone(),
            service: self.service.clone(),
            fallthrough: self.fallthrough.clone(),

            _marker: PhantomData,
        }
    }
}

impl<Fun, Fut, Ser, Inn, Req, Res, Err> Layer<Inn>
    for AsyncFilterLayer<Fun, Fut, Ser, Req, Res, Err>
where
    Fun: Fn(&Req) -> Fut + Clone,
    Fut: Future<Output = bool> + Send + 'static,
    Ser: Service<Req, Response = Res, Error = Err> + Clone,
    Inn: Service<Req, Response = Res, Error = Err>,
{
    type Service = AsyncFilterService<Fun, Fut, Ser, Inn, Req, Res, Err>;

    fn layer(&self, fallthrough: Inn) -> Self::Service {
        let filter = self.filter.clone();
        let service = self.service.clone();

        Self::Service {
            filter,
            service,
            fallthrough,

            _marker: PhantomData,
        }
    }
}

impl<Fun, Fut, Ser, Inn, Req, Res, Err> Service<Req>
    for AsyncFilterService<Fun, Fut, Ser, Inn, Req, Res, Err>
where
    Fun: Fn(&Req) -> Fut + Clone,
    Fut: Future<Output = bool> + Send + 'static,
    Ser: Service<Req, Response = Res, Error = Err> + Clone + Send + 'static,
    Ser::Future: Send + 'static,
    Inn: Service<Req, Response = Res, Error = Err> + Clone + Send + 'static,
    Inn::Future: Send + 'static,
    Req: Send + 'static,
{
    type Response = Res;
    type Error = Err;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.service.poll_ready(cx))?;
        ready!(self.fallthrough.poll_ready(cx))?;

        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let matches = (self.filter)(&req);
        let mut service = self.service.clone();
        let mut fallthrough = self.fallthrough.clone();

        Box::pin(async move {
            if matches.await {
                service.call(req).await
            } else {
                fallthrough.call(req).await
            }
        })
    }
}

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

#[tokio::main]
async fn main() {
    let filter = {
        let state: bool = true;

        move |req: &Request| {
            let matches = req.uri().path().contains("filter");

            async move { matches && state }
        }
    };
    let service = TestService("my_service".into());
    let fallthrough = TestService("fallthrough".into());

    let layer = AsyncFilterLayer::new(filter, service);

    let app = Router::<()>::new()
        .route("/test", get(move || async { "b" }))
        .route("/filter", get(move || async { "c" }))
        .route("/123-filter-asdf", get(move || async { "d" }))
        .layer(layer)
        .nest_service("/nested", fallthrough);

    let server = TestServer::new(app).unwrap();

    let res = server.get("/test").await;
    let text = res.text();

    assert_eq!(text, "b");

    let res = server.get("/filter").await;
    let text = res.text();

    assert_eq!(text, "my_service");

    let res = server.get("/123-filter-asdf").await;
    let text = res.text();

    assert_eq!(text, "my_service");

    println!("It works!");
}
