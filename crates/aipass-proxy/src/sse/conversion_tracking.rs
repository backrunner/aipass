//! Account for delivery conversion while metering the original upstream wire.
use super::*;

pub(crate) fn track_converted_usage_stream<S>(
    source: S,
    context: UsageTrackingContext,
    to: ProxyProtocol,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>>
where
    S: Stream<Item = Result<Bytes, BoxError>> + Send + 'static,
{
    let from = context.protocol;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let source = track_usage_stream_inner(source, context, Some(receiver));
    let converted = convert_sse_stream(source, from, to);
    Box::pin(stream::unfold(
        (Some(converted), Some(sender)),
        move |(mut source, mut outcome)| async move {
            let next = source.as_mut()?.next().await;
            match next {
                Some(result) => {
                    let delivery = match &result {
                        Err(_) => Some(false),
                        Ok(bytes) if stream_reports_error(bytes) => Some(false),
                        Ok(bytes) if sse_event_is_terminal(to, bytes) => Some(true),
                        _ => None,
                    };
                    if let Some(delivered) = delivery {
                        // Releasing the upstream receiver also wakes accounting
                        // when conversion fails before the upstream terminates.
                        drop(source.take());
                        if let Some(sender) = outcome.take() {
                            let (recorded, confirmation) = tokio::sync::oneshot::channel();
                            let _ = sender.send((delivered, recorded));
                            let _ = confirmation.await;
                        }
                    }
                    Some((result, (source, outcome)))
                }
                None => {
                    drop(source.take());
                    if let Some(sender) = outcome.take() {
                        let (recorded, confirmation) = tokio::sync::oneshot::channel();
                        let _ = sender.send((true, recorded));
                        let _ = confirmation.await;
                    }
                    None
                }
            }
        },
    ))
}
