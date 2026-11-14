//! Ordered IPC worker and panel-scoped failures, including startup failures.
use super::{Command, Completion, Job, Reply, Transport};
use std::sync::mpsc::{self, Receiver, Sender};

pub(super) fn start(
    mut transport: impl Transport,
) -> std::io::Result<(Sender<Job>, Receiver<Completion>)> {
    let (sender, jobs) = mpsc::channel::<Job>();
    let (completed, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("desktop-agent-client".into())
        .spawn(move || {
            while let Ok(job) = jobs.recv() {
                let credential = match &job.command {
                    Command::Credential(request) => Some(request.scope),
                    _ => None,
                };
                let trash = match &job.command {
                    Command::Trash(request) => Some(request.generation),
                    _ => None,
                };
                let subscription = match &job.command {
                    Command::Subscription(request) => {
                        Some((request.generation, request.operation.kind()))
                    }
                    _ => None,
                };
                let integration = match &job.command {
                    Command::Integration(request) => Some(request.scope),
                    _ => None,
                };
                let tools = match &job.command {
                    Command::ProviderTools(request) => Some(request.scope),
                    _ => None,
                };
                let pricing = match &job.command {
                    Command::Pricing(request) => Some(request.scope),
                    _ => None,
                };
                let activity = matches!(job.command, Command::Touch);
                let result = transport.execute(job.command);
                let result = match (credential, result) {
                    (Some(scope), Err(error)) => {
                        Ok(Reply::Credential(crate::credentials::Completion {
                            scope,
                            result: Err(error),
                        }))
                    }
                    (_, result) => result,
                };
                let result = match (trash, result) {
                    (Some(generation), Err(error)) => Ok(Reply::Trash(crate::trash::Completion {
                        generation,
                        result: Err(error),
                    })),
                    (_, result) => result,
                };
                let result = match (tools, result) {
                    (Some(scope), Err(error)) => Ok(Reply::ProviderTools(Box::new(
                        crate::provider_tools::Completion {
                            scope,
                            mutated: false,
                            result: Err(error),
                        },
                    ))),
                    (_, result) => result,
                };
                // Startup/transport errors still belong to the requesting panel.
                // Otherwise its pending state would survive without a reply.
                let result = match (subscription, result) {
                    (Some((generation, kind)), Err(error)) => Ok(Reply::Subscription(Box::new(
                        crate::subscriptions::Completion {
                            generation,
                            kind,
                            result: Err(error),
                        },
                    ))),
                    (_, result) => result,
                };
                let result = match (integration, result) {
                    (Some(scope), Err(error)) => Ok(Reply::Integration(Box::new(
                        crate::integration::Completion {
                            scope,
                            result: Err(error),
                        },
                    ))),
                    (_, result) => result,
                };
                let result = match (pricing, result) {
                    (Some(scope), Err(error)) => {
                        Ok(Reply::Pricing(Box::new(crate::pricing::Completion {
                            scope,
                            result: Err(error),
                        })))
                    }
                    (_, result) => result,
                };
                // A view can disappear while authentication is in flight. Still
                // execute its already-queued lock before dropping the transport.
                let _ = completed.send(Completion {
                    generation: job.generation,
                    activity,
                    result,
                });
            }
        })?;
    Ok((sender, receiver))
}
