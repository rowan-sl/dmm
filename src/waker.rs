use futures::future::Future;
use futures::task::{AtomicWaker, Context, Poll};
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

struct Inner {
    waker: AtomicWaker,
    set: AtomicBool,
}

#[derive(Clone)]
pub struct Waker(Arc<Inner>);

impl Waker {
    pub fn new() -> Self {
        Self(Arc::new(Inner {
            waker: AtomicWaker::new(),
            set: AtomicBool::new(false),
        }))
    }

    pub fn wake(&self) {
        self.0.set.store(true, Relaxed);
        self.0.waker.wake();
    }
}

impl Future for Waker {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // quick check to avoid registration if already done.
        if self.0.set.load(Relaxed) {
            self.0.set.store(false, Relaxed);
            return Poll::Ready(());
        }

        self.0.waker.register(cx.waker());

        // Need to check condition **after** `register` to avoid a race
        // condition that would result in lost notifications.
        if self.0.set.load(Relaxed) {
            self.0.set.store(false, Relaxed);
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
