use thiserror::Error;

use std::{
    sync::{atomic::AtomicU64, Arc, Mutex},
    task::{Poll, Waker},
};

use futures::{Sink, Stream};

use super::Publisher;

#[derive(Error, Debug)]
pub enum PublishedValueError {
    #[error("Input argument error: {0}")]
    InvalidInput(String),

    #[error("PublishedValue closed")]
    Closed,
}
#[derive(Clone)]
pub struct PublishedValue<Output>
where
    Output: Sized + Clone,
{
    inner: Arc<Mutex<PublishedValueImpl<Output>>>,
}

impl<Output> PublishedValue<Output>
where
    Output: Sized + Clone,
{
    pub fn new(init_value: Output) -> Self {
        PublishedValue {
            inner: Arc::new(Mutex::new(PublishedValueImpl::new(init_value))),
        }
    }

    pub fn set(&mut self, new_value: Output) -> Result<(), PublishedValueError> {
        self.inner.lock().unwrap().write(new_value)
    }
}

impl<Output> Sink<Output> for PublishedValue<Output>
where
    Output: Sized + Clone,
{
    type Error = PublishedValueError;

    fn start_send(self: std::pin::Pin<&mut Self>, item: Output) -> Result<(), Self::Error> {
        self.inner.lock().unwrap().write(item)?;

        Ok(())
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl<O> Publisher for PublishedValue<O>
where
    O: Sized + Clone,
{
    type Output = O;

    type Failure = PublishedValueError;

    type Stream = PublishedValueStream<O>;

    fn receive(&mut self) -> Self::Stream {
        PublishedValueStream {
            inner: self.inner.clone(),
            current_version: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl<O> Drop for PublishedValue<O>
where
    O: Sized + Clone,
{
    fn drop(&mut self) {
        self.inner.lock().unwrap().closed = true;
    }
}

struct PublishedValueImpl<Output>
where
    Output: Sized + Clone,
{
    value: Output,
    version: u64,
    wakers: Vec<Waker>,
    closed: bool,
}

impl<Output> PublishedValueImpl<Output>
where
    Output: Sized + Clone,
{
    fn new(init: Output) -> Self {
        PublishedValueImpl {
            value: init,
            version: 0,
            wakers: vec![],
            closed: false,
        }
    }

    fn try_read(&mut self, waker: Waker, version: u64) -> Poll<(Option<Output>, u64)> {
        if self.closed {
            return Poll::Ready((None, 0));
        }

        if self.version > version {
            return Poll::Ready((Some(self.value.clone()), self.version));
        }

        self.wakers.push(waker);

        return Poll::Pending;
    }

    fn write(&mut self, value: Output) -> Result<(), PublishedValueError> {
        if self.closed {
            return Err(PublishedValueError::Closed);
        }

        self.value = value;
        self.version += 1;

        for waker in &self.wakers {
            waker.wake_by_ref();
        }

        self.wakers.clear();

        Ok(())
    }
}

#[derive(Clone)]
pub struct PublishedValueStream<Output>
where
    Output: Sized + Clone,
{
    inner: Arc<Mutex<PublishedValueImpl<Output>>>,
    current_version: Arc<AtomicU64>,
}

impl<Output> Drop for PublishedValueStream<Output>
where
    Output: Sized + Clone,
{
    fn drop(&mut self) {
        drop(&self.inner)
    }
}

impl<Output> Stream for PublishedValueStream<Output>
where
    Output: Sized + Clone,
{
    type Item = Output;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let version = self
            .current_version
            .load(std::sync::atomic::Ordering::SeqCst);

        match self
            .inner
            .lock()
            .unwrap()
            .try_read(cx.waker().clone(), version)
        {
            Poll::Ready((item, new_version)) => {
                _ = self.current_version.compare_exchange(
                    version,
                    new_version,
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                );

                return Poll::Ready(item);
            }
            _ => return Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use futures::prelude::*;

    #[async_std::test]
    async fn test_published_value() -> Result<(), anyhow::Error> {
        pretty_env_logger::init();

        let mut number = PublishedValue::new(1_i32);

        let mut subscriber = number.receive().map(|x| x * 2);

        async_std::task::spawn(async move {
            for i in 1..10 {
                log::debug!("send {}", i);
                number.send(i).await.unwrap();
            }

            drop(number);
        });

        while let Some(data) = subscriber.next().await {
            log::debug!("{}", data);
        }

        Ok(())
    }
}
