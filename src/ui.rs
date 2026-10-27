use crate::Result;
use cli_command_kit::Mode;
use std::{io::Write, sync::Mutex};
pub struct Ui {
    pub mode: Mode,
    writer: Mutex<Box<dyn Write + Send>>,
}
impl Ui {
    pub fn new(mode: Mode, writer: impl Write + Send + 'static) -> Self {
        Self {
            mode,
            writer: Mutex::new(Box::new(writer)),
        }
    }
    pub fn line(&self, text: impl AsRef<str>) -> Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| std::io::Error::other("output lock poisoned"))?;
        writeln!(writer, "{}", Self::fold_text(text.as_ref()))?;
        writer.flush()?;
        Ok(())
    }
    pub fn write(&self, text: &str) -> Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| std::io::Error::other("output lock poisoned"))?;
        writer.write_all(Self::fold_text(text).as_bytes())?;
        writer.flush()?;
        Ok(())
    }
    pub fn status(&self, key: &str, value: impl std::fmt::Display) -> Result<()> {
        self.line(format!("{key}: {value}"))
    }
    pub fn path(path: &std::path::Path) -> String {
        cli_command_kit::display_path(path)
    }
    pub fn fold_text(text: &str) -> String {
        cli_command_kit::display_message(text)
    }
}
pub struct LiveOutput<'a> {
    ui: &'a Ui,
    pending: [Vec<u8>; 2],
}
impl<'a> LiveOutput<'a> {
    pub fn new(ui: &'a Ui) -> Self {
        Self {
            ui,
            pending: Default::default(),
        }
    }
    pub fn feed(&mut self, bytes: &[u8], stderr: bool) -> Result<()> {
        let pending = &mut self.pending[usize::from(stderr)];
        pending.extend_from_slice(bytes);
        while let Some(end) = pending.iter().position(|b| *b == b'\n') {
            self.ui.write(&String::from_utf8_lossy(&pending[..=end]))?;
            pending.drain(..=end);
        }
        Ok(())
    }
    pub fn finish(self) -> Result<()> {
        for pending in self.pending {
            if !pending.is_empty() {
                self.ui.write(&String::from_utf8_lossy(&pending))?
            }
        }
        Ok(())
    }
}
pub use cli_command_kit::fold_message as fold_text_home;
