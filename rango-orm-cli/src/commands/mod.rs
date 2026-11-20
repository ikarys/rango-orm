pub mod dump;
pub mod export;
pub mod import;
pub mod init;
pub mod makemigrations;
pub mod migrate;

/// Print `text` to stdout; a closed pipe (`rango export | head`) ends quietly instead of panicking.
pub(crate) fn print_stdout(text: &str) -> anyhow::Result<()> {
    use std::io::{ErrorKind, Write};
    let mut out = std::io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Err(e) if e.kind() != ErrorKind::BrokenPipe => Err(e.into()),
        _ => Ok(()),
    }
}
