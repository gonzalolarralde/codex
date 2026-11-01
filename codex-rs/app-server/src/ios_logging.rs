use std::io::Write;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Copy)]
pub(crate) struct IosLogMakeWriter;

#[derive(Default)]
pub(crate) struct IosLogWriter {
    buffer: Vec<u8>,
}

impl<'a> MakeWriter<'a> for IosLogMakeWriter {
    type Writer = IosLogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        IosLogWriter::default()
    }
}

impl Write for IosLogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(bytes);
        while let Some(newline) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line = self.buffer.drain(..=newline).collect::<Vec<_>>();
            Self::emit_line(&line[..line.len().saturating_sub(1)]);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flush_buffer();
        Ok(())
    }
}

impl Drop for IosLogWriter {
    fn drop(&mut self) {
        self.flush_buffer();
    }
}

impl IosLogWriter {
    fn flush_buffer(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let line = std::mem::take(&mut self.buffer);
        Self::emit_line(&line);
    }

    fn emit_line(bytes: &[u8]) {
        let message = String::from_utf8_lossy(bytes);
        codex_ios_platform::log_message("debug", "codex.app-server.stderr", message.as_ref());
    }
}
