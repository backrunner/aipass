use aipass_native_host::{handle_request, read_message, write_message};
use anyhow::{Context, Result};
use std::io::{stdin, stdout, ErrorKind, Write};

fn main() -> Result<()> {
    let stdin = stdin();
    let stdout = stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    loop {
        let request = match read_message(&mut reader) {
            Ok(request) => request,
            Err(err)
                if err
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == ErrorKind::UnexpectedEof) =>
            {
                break;
            }
            Err(err) => return Err(err).context("failed to read native message"),
        };
        let response = handle_request(request);
        write_message(&mut writer, &response)?;
        writer.flush()?;
    }
    Ok(())
}
