use fetch_mix_cli::ui::{LiveOutput, Ui, fold_text_home};
use std::{
    io::{self, Write},
    path::Path,
    sync::{Arc, Mutex},
};
#[derive(Clone)]
struct Buffer(Arc<Mutex<Vec<u8>>>);
impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct Broken;
impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn line_streaming_home_folding_and_output_errors() {
    let home = Path::new("/home/person");
    assert_eq!(fold_text_home("/home/person", home), "~");
    assert_eq!(
        fold_text_home("path: /home/person/music and /home/person2/music", home),
        "path: ~/music and /home/person2/music"
    );
    assert_eq!(fold_text_home("/outside/music", home), "/outside/music");
    assert_eq!(
        fold_text_home("[/home/person/music]; '/home/person'", home),
        "[~/music]; '~'"
    );
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let ui = Ui::new(cli_command_kit::Mode::Agent, Buffer(bytes.clone()));
    let mut live = LiveOutput::new(&ui);
    live.feed(b"first", false).unwrap();
    assert!(bytes.lock().unwrap().is_empty());
    live.feed(b" line\nlast", false).unwrap();
    live.feed(b"error", true).unwrap();
    assert_eq!(&*bytes.lock().unwrap(), b"first line\n");
    live.finish().unwrap();
    assert_eq!(&*bytes.lock().unwrap(), b"first line\nlasterror");
    let ui = Ui::new(cli_command_kit::Mode::Human, Broken);
    assert!(ui.line("broken").is_err());
    assert!(ui.write("broken").is_err());
    let mut live = LiveOutput::new(&ui);
    live.feed(b"incomplete", false).unwrap();
    assert!(live.finish().is_err());
}
