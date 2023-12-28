use portable_pty::{CommandBuilder, PtySize, native_pty_system, PtySystem, Child, PtyPair};
use std::{sync::mpsc, thread};

pub struct Commands {
    io_rx: mpsc::Receiver<String>,
    //reader: Box<dyn std::io::Read + Send>,
    pty_pair: PtyPair,
    writer: Box<dyn std::io::Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    output: Vec<String>
}

impl Commands {
    pub fn new() -> Self {
        let pty_system = native_pty_system();

        // Create a new pty
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            // Not all systems support pixel_width, pixel_height,
            // but it is good practice to set it to something
            // that matches the size of the selected font.  That
            // is more complex than can be shown here in this
            // brief example though!
            pixel_width: 0,
            pixel_height: 0,
        }).unwrap();

        // Spawn a shell into the pty (TODO: find the default shell)
        let cmd = CommandBuilder::new("zsh");
        let child = pair.slave.spawn_command(cmd).unwrap();

        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut writer = pair.master.take_writer().unwrap();

        writer.write_all(b"vim\r\n\0");

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf: Vec<u8> = vec![0; 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(n) => if n > 0 {
                        let _ = tx.send(
                            std::str::from_utf8(&buf[0..n]).unwrap_or("").to_string()
                        );
                    }
                    Err(_) => {}
                }
            }
        });

        Self {
            io_rx: rx,
            //reader,
            pty_pair: pair,
            writer,
            child,
            output: vec![]
        }
    }

    pub fn poll_io(&mut self) {
        while let Ok(v) = self.io_rx.try_recv() {
            self.output.push(v);
        }
    }

    pub fn resize(&mut self, rows: u32, cols: u32) {
        let _ = self.pty_pair.master.resize(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0
        });
    }

    pub fn send_input(&mut self, input: &Vec<u8>) {
        let _ = self.writer.write_all(input);
    }

    pub fn get_output(&self) -> &Vec<String> {
        &self.output
    }

    pub fn clear_output(&mut self) {
        self.output.clear();
    }
}
