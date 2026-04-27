use std::io::Write;

use crate::tmux::control_process::ControlModeProcess;
use crate::tmux::control_protocol::{command_line, is_control_transport_error, read_control_block};
use crate::tmux::polling::PollingError;

#[derive(Debug)]
pub(crate) struct ControlModeClient {
    process: ControlModeProcess,
}

impl ControlModeClient {
    pub(crate) fn attach_current() -> Result<Self, PollingError> {
        let mut client = Self {
            process: ControlModeProcess::attach_current()?,
        };
        let _ = client.read_response()?;
        Ok(client)
    }

    pub(crate) fn run_command(&mut self, args: &[String]) -> Result<Vec<String>, PollingError> {
        writeln!(self.process.stdin, "{}", command_line(args))
            .map_err(|e| PollingError::Command(e.to_string()))?;
        self.process
            .stdin
            .flush()
            .map_err(|e| PollingError::Command(e.to_string()))?;
        self.read_response()
    }

    fn read_response(&mut self) -> Result<Vec<String>, PollingError> {
        read_control_block(&mut self.process.stdout)
    }
}

pub(crate) fn run_client_with_reconnect(
    client: &mut ControlModeClient,
    args: &[String],
) -> Result<Vec<String>, PollingError> {
    run_with_reconnect(client, args, ControlModeClient::attach_current)
}

trait ControlModeCommandRunner {
    fn run_command(&mut self, args: &[String]) -> Result<Vec<String>, PollingError>;
}

impl ControlModeCommandRunner for ControlModeClient {
    fn run_command(&mut self, args: &[String]) -> Result<Vec<String>, PollingError> {
        Self::run_command(self, args)
    }
}

fn run_with_reconnect<C, F>(
    client: &mut C,
    args: &[String],
    reconnect: F,
) -> Result<Vec<String>, PollingError>
where
    C: ControlModeCommandRunner,
    F: FnOnce() -> Result<C, PollingError>,
{
    match client.run_command(args) {
        Ok(output) => Ok(output),
        Err(first) if is_control_transport_error(&first) => {
            *client = reconnect().map_err(|reconnect| {
                PollingError::Command(format!("control-mode reconnect after {first}: {reconnect}"))
            })?;
            client.run_command(args)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    #[derive(Debug)]
    struct ScriptedClient {
        responses: VecDeque<Result<Vec<String>, PollingError>>,
        call_count: Arc<AtomicUsize>,
    }

    impl ScriptedClient {
        fn new(responses: Vec<Result<Vec<String>, PollingError>>) -> Self {
            Self {
                responses: responses.into(),
                call_count: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn with_counter(
            responses: Vec<Result<Vec<String>, PollingError>>,
            call_count: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                responses: responses.into(),
                call_count,
            }
        }
    }

    impl ControlModeCommandRunner for ScriptedClient {
        fn run_command(&mut self, _args: &[String]) -> Result<Vec<String>, PollingError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            self.responses
                .pop_front()
                .unwrap_or_else(|| Err(PollingError::Command("script exhausted".into())))
        }
    }

    #[test]
    fn lifecycle_error_reconnects_once_and_retries_command() {
        let first_counter = Arc::new(AtomicUsize::new(0));
        let retry_counter = Arc::new(AtomicUsize::new(0));
        let mut client = ScriptedClient::with_counter(
            vec![Err(PollingError::NonZero("%exit detached".into()))],
            first_counter.clone(),
        );
        let args = vec!["list-panes".into()];

        let output = run_with_reconnect(&mut client, &args, || {
            Ok(ScriptedClient::with_counter(
                vec![Ok(vec!["ok".into()])],
                retry_counter.clone(),
            ))
        })
        .unwrap();

        assert_eq!(output, vec!["ok"]);
        assert_eq!(first_counter.load(Ordering::SeqCst), 1);
        assert_eq!(retry_counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn command_error_does_not_reconnect() {
        let mut client = ScriptedClient::new(vec![Err(PollingError::NonZero("bad target".into()))]);
        let args = vec!["list-panes".into()];
        let reconnect_calls = Arc::new(AtomicUsize::new(0));

        let err = run_with_reconnect(&mut client, &args, || {
            reconnect_calls.fetch_add(1, Ordering::SeqCst);
            Ok(ScriptedClient::new(vec![Ok(vec!["unexpected".into()])]))
        })
        .unwrap_err();

        assert_eq!(err.to_string(), "tmux returned non-zero: bad target");
        assert_eq!(reconnect_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn reconnect_failure_includes_original_lifecycle_error() {
        let mut client =
            ScriptedClient::new(vec![Err(PollingError::Command("Broken pipe".into()))]);
        let args = vec!["list-panes".into()];

        let err = run_with_reconnect(&mut client, &args, || {
            Err(PollingError::Command("tmux missing".into()))
        })
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "tmux command failed: control-mode reconnect after tmux command failed: Broken pipe: tmux command failed: tmux missing"
        );
    }
}
