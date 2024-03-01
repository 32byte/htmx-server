use std::{
    convert::Infallible,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use axum::{extract::Request, response::Response, routing::get, Router};
use axum_test::TestServer;
use futures::{
    future::{ready, Either, Ready},
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
    type Future = MyFuture<Fut, Ser, Inn, Req, Res, Err>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.service.poll_ready(cx))?;
        ready!(self.fallthrough.poll_ready(cx))?;

        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let matches = (self.filter)(&req);
        let service = self.service.clone();
        let fallthrough = self.fallthrough.clone();

        MyFuture::new(matches, service, fallthrough, req)
    }
}

#[pin_project::pin_project]
pub struct MyFuture<Fil, Left, Right, Req, Res, Err>
where
    Fil: Future<Output = bool>,
    Left: Service<Req, Response = Res, Error = Err>,
    Right: Service<Req, Response = Res, Error = Err>,
{
    #[pin]
    filter: Fil,

    #[pin]
    future: Option<Either<Left::Future, Right::Future>>,

    req: Option<Req>,
    left: Left,
    right: Right,

    _marker: PhantomData<(Res, Err)>,
}

impl<Fil, Left, Right, Req, Res, Err> MyFuture<Fil, Left, Right, Req, Res, Err>
where
    Fil: Future<Output = bool>,
    Left: Service<Req, Response = Res, Error = Err>,
    Right: Service<Req, Response = Res, Error = Err>,
{
    pub fn new(filter: Fil, left: Left, right: Right, req: Req) -> Self {
        Self {
            filter,
            future: None,

            req: Some(req),
            left,
            right,

            _marker: PhantomData,
        }
    }
}

impl<Fil, Left, Right, Req, Res, Err> Future for MyFuture<Fil, Left, Right, Req, Res, Err>
where
    Fil: Future<Output = bool>,
    Left: Service<Req, Response = Res, Error = Err>,
    Right: Service<Req, Response = Res, Error = Err>,
{
    type Output = Result<Res, Err>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        if let Some(future) = this.future.as_mut().as_pin_mut() {
            return future.poll(cx);
        }

        let matches = ready!(this.filter.poll(cx));

        let req = this.req.take().expect("Request was none!");

        let _fut = if matches {
            Either::Left(this.left.call(req))
        } else {
            Either::Right(this.right.call(req))
        };

        unsafe {
            let future_ref = this.future.as_mut();
            let future = Pin::get_unchecked_mut(future_ref);

            *future = Some(_fut);
        }

        this.future
            .as_mut()
            .as_pin_mut()
            .expect("I just set the future")
            .poll(cx)
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
