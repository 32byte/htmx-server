use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures::{future::Either, ready, Future};
use tower::Service;

#[pin_project::pin_project]
pub struct SelectServiceAndCallFut<C, A, B, T, R, E>
where
    C: Future<Output = bool>,

    A: Service<T, Response = R, Error = E>,
    B: Service<T, Response = R, Error = E>,
{
    #[pin]
    condition: C,

    // TODO: I think I can represent this as an enum, so I don't have to
    //       maintain the invariants myself.
    // INV: This is Some(...) when future is None
    value: Option<T>,

    // INV: This is Some(...) when future is None
    services: Option<(A, B)>,

    #[pin]
    future: Option<Either<A::Future, B::Future>>,
}

impl<C, A, B, T, R, E> SelectServiceAndCallFut<C, A, B, T, R, E>
where
    C: Future<Output = bool>,

    A: Service<T, Response = R, Error = E>,
    B: Service<T, Response = R, Error = E>,
{
    pub fn new(condition: C, value: T, service_a: A, service_b: B) -> Self {
        Self {
            condition,
            value: Some(value),
            future: None,
            services: Some((service_a, service_b)),
        }
    }
}

impl<C, A, B, T, R, E> Future for SelectServiceAndCallFut<C, A, B, T, R, E>
where
    C: Future<Output = bool>,

    A: Service<T, Response = R, Error = E>,
    B: Service<T, Response = R, Error = E>,
{
    type Output = Result<R, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        if let Some(future) = this.future.as_mut().as_pin_mut() {
            return future.poll(cx);
        }

        let select = ready!(this.condition.poll(cx));

        let value = this
            .value
            .take()
            .expect("Invariant violation: value is None when future is None");

        let (mut service_a, mut service_b) = this
            .services
            .take()
            .expect("Invariant violation: services is None when future is None");

        let fut = if select {
            Either::Left(service_a.call(value))
        } else {
            Either::Right(service_b.call(value))
        };

        this.future.as_mut().set(Some(fut));

        this.future
            .as_mut()
            .as_pin_mut()
            .expect("I just set the future :)")
            .poll(cx)
    }
}

#[cfg(test)]
mod tests {
    use futures::future::ready;

    use super::*;
    use crate::test_util::*;

    #[tokio::test]
    async fn should_select_first() {
        let first = TestService("first");
        let second = TestService("second");

        let fut = SelectServiceAndCallFut::new(ready(true), "value", first, second);

        let res = fut.await.unwrap();

        assert_eq!(res, "first");
    }

    #[tokio::test]
    async fn should_select_second() {
        let first = TestService("first");
        let second = TestService("second");

        let fut = SelectServiceAndCallFut::new(ready(false), "value", first, second);

        let res = fut.await.unwrap();

        assert_eq!(res, "second");
    }
}
