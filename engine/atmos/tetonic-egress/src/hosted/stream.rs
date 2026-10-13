//! A live model stream may outlast two minutes. Bound silence independently
//! from total transport lifetime; the caller can still cancel sooner.
use std::{future::Future, time::Duration};

use futures_util::{Stream, StreamExt};
use tokio::time::{timeout, timeout_at, Instant};

use super::{failure, EgressError, MAX_BODY};

const IDLE_LIMIT: Duration = Duration::from_secs(120);
const TOTAL_LIMIT: Duration = Duration::from_secs(900);

pub(super) async fn receive<F, S, B, E>(
    open: F,
    on_chunk: &mut (dyn FnMut(&[u8]) -> Result<(), EgressError> + Send),
) -> Result<(), EgressError>
where
    F: Future<Output = Result<S, EgressError>>,
    S: Stream<Item = Result<B, E>>,
    B: AsRef<[u8]>,
{
    timeout(TOTAL_LIMIT, async {
        let stream = timeout(IDLE_LIMIT, open)
            .await
            .map_err(|_| failure("hosted stream timed out waiting for response headers"))??;
        futures_util::pin_mut!(stream);
        let mut received = 0usize;
        let mut idle_deadline = Instant::now() + IDLE_LIMIT;
        loop {
            let Some(chunk) = timeout_at(idle_deadline, stream.next())
                .await
                .map_err(|_| failure("hosted stream timed out waiting for more data"))?
            else {
                return Ok(());
            };
            let chunk = chunk.map_err(|_| failure("hosted stream interrupted"))?;
            let bytes = chunk.as_ref();
            received = received.saturating_add(bytes.len());
            if received > MAX_BODY {
                return Err(failure("hosted stream exceeds 8 MiB"));
            }
            if !bytes.is_empty() {
                idle_deadline = Instant::now() + IDLE_LIMIT;
                on_chunk(bytes)?;
            }
        }
    })
    .await
    .map_err(|_| failure("hosted stream timed out at the 15 minute transport limit"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;

    #[tokio::test(start_paused = true)]
    async fn progressing_stream_can_outlast_two_minutes() {
        let chunks = async_stream::stream! {
            for _ in 0..4 {
                tokio::time::sleep(Duration::from_secs(60)).await;
                yield Ok::<_, ()>(b"data: live\n\n".as_slice());
            }
        };
        let start = Instant::now();
        let mut count = 0;
        receive(async { Ok(chunks) }, &mut |_| {
            count += 1;
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(count, 4);
        assert_eq!(start.elapsed(), Duration::from_secs(240));
    }

    #[tokio::test(start_paused = true)]
    async fn silent_stream_and_missing_headers_are_bounded() {
        let error = receive(
            async { Ok(stream::pending::<Result<Vec<u8>, ()>>()) },
            &mut |_| Ok(()),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("waiting for more data"));
        let error = receive(
            async {
                std::future::pending::<()>().await;
                Ok(stream::empty::<Result<Vec<u8>, ()>>())
            },
            &mut |_| Ok(()),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("waiting for response headers"));
    }

    #[tokio::test(start_paused = true)]
    async fn endless_heartbeats_still_reach_total_limit() {
        let chunks = async_stream::stream! {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                yield Ok::<_, ()>(b": heartbeat\n\n".as_slice());
            }
        };
        let start = Instant::now();
        let error = receive(async { Ok(chunks) }, &mut |_| Ok(()))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("15 minute transport limit"));
        assert_eq!(start.elapsed(), TOTAL_LIMIT);
    }

    #[tokio::test(start_paused = true)]
    async fn empty_chunks_do_not_keep_a_silent_stream_alive() {
        let chunks = async_stream::stream! {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                yield Ok::<_, ()>(Vec::<u8>::new());
            }
        };
        let error = receive(async { Ok(chunks) }, &mut |_| panic!("empty chunk"))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("waiting for more data"));
    }

    #[tokio::test]
    async fn size_decode_and_transport_errors_remain_bounded_without_retries() {
        let error = receive(
            async { Ok(stream::iter([Ok::<_, ()>(vec![0; MAX_BODY + 1])])) },
            &mut |_| Ok(()),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("exceeds 8 MiB"));
        let mut calls = 0;
        let error = receive(
            async { Ok(stream::iter([Ok::<_, ()>(b"event"), Ok(b"again")])) },
            &mut |_| {
                calls += 1;
                Err(failure("invalid protocol event"))
            },
        )
        .await
        .unwrap_err();
        assert_eq!(calls, 1);
        assert!(error.to_string().contains("invalid protocol event"));
        let error = receive(
            async { Ok(stream::iter([Err::<Vec<u8>, _>(())])) },
            &mut |_| Ok(()),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("interrupted"));
    }

    #[tokio::test(start_paused = true)]
    async fn caller_cancellation_drops_the_stream() {
        let (send, receive_drop) = tokio::sync::oneshot::channel();
        struct Dropped(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                let _ = self.0.take().unwrap().send(());
            }
        }
        let chunks = async_stream::stream! {
            let _dropped = Dropped(Some(send));
            std::future::pending::<()>().await;
            yield Ok::<_, ()>(Vec::<u8>::new());
        };
        assert!(timeout(
            Duration::from_secs(1),
            receive(async { Ok(chunks) }, &mut |_| Ok(()))
        )
        .await
        .is_err());
        receive_drop.await.unwrap();
    }
}
