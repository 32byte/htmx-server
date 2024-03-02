use std::future::Future;
use std::marker::PhantomData;
use std::task::{Context, Poll};

use futures::ready;
use tower::{Layer, Service};

use crate::futures::SelectServiceAndCallFut;

/// A filter that allows a service to be executed based on a condition
///
/// # Example
/// ```rust
/// # use tower_fallthrough_filter::AsyncFilter;
/// # use futures::future::BoxFuture;
///
/// #[derive(Debug, Clone)]
/// struct MyFilter;
///
/// impl<T> AsyncFilter<T> for MyFilter {
///     type Future = BoxFuture<'static, bool>;
///
///     fn matches(&self, _: &T) -> Self::Future {
///         Box::pin(async move { true })
///     }
/// }
///
/// # #[tokio::main]
/// # async fn main() {
/// let filter = MyFilter;
/// assert_eq!(filter.matches(&()).await, true);
/// # }
/// ```
pub trait AsyncFilter<T>: Clone + Send + Sync {
    type Future: Future<Output = bool> + Send;

    fn matches(&self, item: &T) -> Self::Future;
}

pub struct AsyncFilterLayer<F, S, T, R, E>
where
    F: AsyncFilter<T>,
    S: Service<T, Response = R, Error = E>,
{
    filter: F,
    service: S,

    _marker: PhantomData<(T, R, E)>,
}

// NOTE: This is required to make the `FilterLayer` clonable
//       as the `PhantomData` might be not clonable.
impl<F, S, R, E, T> Clone for AsyncFilterLayer<F, S, T, R, E>
where
    F: AsyncFilter<T> + Clone,
    S: Service<T, Response = R, Error = E> + Clone,
{
    fn clone(&self) -> Self {
        Self {
            filter: self.filter.clone(),
            service: self.service.clone(),

            _marker: PhantomData,
        }
    }
}

impl<F: AsyncFilter<T>, S: Service<T>, T: Send + 'static>
    AsyncFilterLayer<F, S, T, S::Response, S::Error>
{
    /// Creates a new FilterLayer given a `Service` and a `Filter`.
    ///
    /// NOTE: The Service and the Filter have to operate on the same
    /// type `T`.
    pub fn new(filter: F, service: S) -> Self {
        Self {
            filter,
            service,

            _marker: PhantomData,
        }
    }
}

impl<F, S, I, T, R, E> Layer<I> for AsyncFilterLayer<F, S, T, R, E>
where
    F: AsyncFilter<T> + Clone,
    S: Service<T, Response = R, Error = E> + Clone,
    I: Service<T, Response = R, Error = E> + Clone,
    T: Send + 'static,
{
    type Service = AsyncFilterService<F, S, I, T, R, E>;

    fn layer(&self, inner_service: I) -> Self::Service {
        let filter = self.filter.clone();
        let filtered_service = self.service.clone();

        AsyncFilterService {
            filter,
            service: filtered_service,
            inner: inner_service,

            _marker: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct AsyncFilterService<F, S, I, T, R, E>
where
    F: AsyncFilter<T>,
    S: Service<T, Response = R, Error = E>,
    I: Service<T, Response = R, Error = E>,
{
    filter: F,
    service: S,
    inner: I,

    _marker: PhantomData<(T, R, E)>,
}

// NOTE: This is required to make the `FilterService` clonable
//       as the `PhantomData` might be not clonable.
impl<F, S, I, T, R, E> Clone for AsyncFilterService<F, S, I, T, R, E>
where
    F: AsyncFilter<T>,
    S: Service<T, Response = R, Error = E> + Clone,
    I: Service<T, Response = R, Error = E> + Clone,
{
    fn clone(&self) -> Self {
        Self {
            filter: self.filter.clone(),
            service: self.service.clone(),
            inner: self.inner.clone(),

            _marker: PhantomData,
        }
    }
}

impl<F, S, I, T, R, E> Service<T> for AsyncFilterService<F, S, I, T, R, E>
where
    F: AsyncFilter<T>,
    F::Future: Send + 'static,
    S: Service<T, Response = R, Error = E> + Clone + Send + 'static,
    S::Future: Send + 'static,
    I: Service<T, Response = R, Error = E> + Clone + Send + 'static,
    I::Future: Send + 'static,
    T: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = SelectServiceAndCallFut<F::Future, S, I, T, R, E>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.service.poll_ready(cx))?;
        // NOTE: It is probably best to poll the `inner_service` here as well
        //       as otherwise it might be called when it isn't ready yet.
        ready!(self.inner.poll_ready(cx))?;

        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: T) -> Self::Future {
        let matches = self.filter.matches(&req);
        let service = self.service.clone();
        let inner = self.inner.clone();
        // TODO: std::mem::replace the services as the clone might not be ready

        SelectServiceAndCallFut::new(matches, req, service, inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::*;

    #[tokio::test]
    async fn should_allow() {
        let service_a = TestService("a");
        let service_b = TestService("b");

        let filter = TestFilter(true);
        let filter_layer = AsyncFilterLayer::new(filter, service_a);

        let mut middleware = filter_layer.layer(service_b);

        assert_eq!(middleware.call(()).await, Ok("a"));
    }

    #[tokio::test]
    async fn should_fall_through() {
        let service_a = TestService("a");
        let service_b = TestService("b");

        let filter = TestFilter(false);
        let filter_layer = AsyncFilterLayer::new(filter, service_a);

        let mut middleware = filter_layer.layer(service_b);

        assert_eq!(middleware.call(()).await, Ok("b"));
    }
}
