use std::{
    convert::Infallible,
    task::{Context, Poll},
};

use futures::future::{ready, Ready};
use tower::Service;

use crate::Filter;

#[cfg(feature = "async")]
use crate::AsyncFilter;

#[derive(Debug)]
pub struct TestService<T>(pub T);

impl<T: Clone> Clone for TestService<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Clone, R> Service<R> for TestService<T> {
    type Response = T;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: R) -> Self::Future {
        ready(Ok(self.0.clone()))
    }
}

#[derive(Debug, Clone)]
pub struct TestFilter(pub bool);

impl<T> Filter<T> for TestFilter {
    fn matches(&self, _: &T) -> bool {
        self.0
    }
}

#[cfg(feature = "async")]
impl<T> AsyncFilter<T> for TestFilter {
    type Future = Ready<bool>;

    fn matches(&self, _: &T) -> Self::Future {
        ready(self.0)
    }
}
