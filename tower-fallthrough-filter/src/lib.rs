use std::{
    marker::PhantomData,
    task::{Context, Poll},
};

use futures::{future::Either, ready};
use tower::{Layer, Service};

/// A filter that allows a service to be executed based on a condition
///
/// # Example
/// ```rust
/// # use tower_fallthrough_filter::Filter;
///
/// #[derive(Debug, Clone)]
/// struct MyFilter;
///
/// impl<T> Filter<T> for MyFilter {
///     fn matches(&self, _: &T) -> bool {
///         true
///     }
/// }
///
/// let filter = MyFilter;
/// assert_eq!(filter.matches(&()), true);
/// ```
pub trait Filter<T>: Clone {
    /// Whether the service should be executed
    ///
    /// If `true`, the service will be executed,  otherwise it will
    /// fall through to the next service.
    fn matches(&self, item: &T) -> bool;
}

/// A Tower layer that executes the provided service only
/// if the given filter returns true.
/// Otherwise it falls through to the inner server.
///
/// # Example
/// ```rust
/// use tower_fallthrough_filter::{Filter, FilterLayer};
/// use tower::{Service, Layer};
///
/// #[derive(Debug, Clone)]
/// struct MyFilter;
///
/// impl Filter<bool> for MyFilter {
///     fn matches(&self, data: &bool) -> bool {
///         *data
///     }
/// }
///
/// #[derive(Debug, Clone)]
/// struct StringService(String);
///
/// impl Service<bool> for StringService {
///     type Response = String;
///     type Error = std::convert::Infallible;
///     type Future = std::future::Ready::<Result<Self::Response, Self::Error>>;
///
///     fn poll_ready(
///         &mut self,
///         _: &mut std::task::Context<'_>,
///     ) -> std::task::Poll<Result<(), Self::Error>> {
///         std::task::Poll::Ready(Ok(()))
///     }
///
///     fn call(&mut self, req: bool) -> Self::Future {
///         std::future::ready(Ok(self.0.clone()))
///     }
/// }
///
/// #[tokio::main]
/// async fn main() {
///     let service_a = StringService("A".to_string());
///     let service_b = StringService("B".to_string());
///     let filter = MyFilter;
///
///     let mut middleware = FilterLayer::new(filter, service_a).layer(service_b);
///
///     assert_eq!(middleware.call(true).await, Ok("A".to_string()));
///     assert_eq!(middleware.call(false).await, Ok("B".to_string()));
/// }
///
#[derive(Debug)]
pub struct FilterLayer<F, S, T, R, E>
where
    F: Filter<T>,
    S: Service<T, Response = R, Error = E>,
{
    filter: F,
    service: S,

    _marker: PhantomData<(T, R, E)>,
}

// NOTE: This is required to make the `FilterLayer` clonable
//       as the `PhantomData` might be not clonable.
impl<F, S, R, E, T> Clone for FilterLayer<F, S, T, R, E>
where
    F: Filter<T> + Clone,
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

impl<F: Filter<T>, S: Service<T>, T> FilterLayer<F, S, T, S::Response, S::Error> {
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

impl<F, S, I, T, R, E> Layer<I> for FilterLayer<F, S, T, R, E>
where
    F: Filter<T> + Clone,
    S: Service<T, Response = R, Error = E> + Clone,
    I: Service<T, Response = R, Error = E> + Clone,
{
    type Service = FilterService<F, S, I, T, R, E>;

    fn layer(&self, inner_service: I) -> Self::Service {
        let filter = self.filter.clone();
        let filtered_service = self.service.clone();

        FilterService {
            filter,
            service: filtered_service,
            inner: inner_service,

            _marker: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct FilterService<F, S, I, T, R, E>
where
    F: Filter<T>,
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
impl<F, S, I, T, R, E> Clone for FilterService<F, S, I, T, R, E>
where
    F: Filter<T>,
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

impl<F, S, I, T, R, E> Service<T> for FilterService<F, S, I, T, R, E>
where
    F: Filter<T>,
    S: Service<T, Response = R, Error = E>,
    S::Future: Send + 'static,
    I: Service<T, Response = R, Error = E>,
    I::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Either<S::Future, I::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.service.poll_ready(cx))?;
        // NOTE: It is probably best to poll the `inner_service` here as well
        //       as otherwise it might be called when it isn't ready yet.
        ready!(self.inner.poll_ready(cx))?;

        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: T) -> Self::Future {
        if self.filter.matches(&req) {
            Either::Left(self.service.call(req))
        } else {
            Either::Right(self.inner.call(req))
        }
    }
}

#[cfg(feature = "async")]
pub use async_feature::{AsyncFilter, AsyncFilterLayer, AsyncFilterService};

#[cfg(feature = "async")]
mod async_feature {
    use std::future::Future;
    use std::marker::PhantomData;
    use std::task::{Context, Poll};

    use futures::future::BoxFuture;
    use futures::ready;
    use tower::{Layer, Service};

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
        // TODO: It should be possible to use a custom future here
        //       and not use `BoxFuture`.
        type Future = BoxFuture<'static, Result<R, E>>;

        fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            ready!(self.service.poll_ready(cx))?;
            // NOTE: It is probably best to poll the `inner_service` here as well
            //       as otherwise it might be called when it isn't ready yet.
            ready!(self.inner.poll_ready(cx))?;

            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: T) -> Self::Future {
            let matches = self.filter.matches(&req);
            let mut service = self.service.clone();
            let mut inner = self.inner.clone();

            Box::pin(async move {
                if matches.await {
                    service.call(req).await
                } else {
                    inner.call(req).await
                }
            })
        }
    }
}
