use portable_pty::{CommandBuilder, PtySize, native_pty_system, Child, PtyPair};
use std::thread;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, mpsc,
};

use crate::config;

#[derive(PartialEq, Eq, Debug)]
enum ShellState {
    Init,
    PromptIdA,
    PromptIdB,
    Ready,
}

pub struct Commands {
    io_rx: mpsc::Receiver<Vec<u8>>,
    pty_pair: PtyPair,
    writer: Box<dyn std::io::Write + Send>,
    //reader: Box<dyn std::io::Read + Send>,
    child: Box<dyn Child + Send + Sync>,
    active: Arc<AtomicBool>,
    output: Vec<u8>,
    rows: u32,
    cols: u32,

}

impl Commands {
    pub fn new() -> Self {
        let pty_system = native_pty_system();

        // Create a new pty
        let default_rows = 24;
        let default_cols = 80;
        let pair = pty_system.openpty(PtySize {
            rows: default_rows,
            cols: default_cols,
            // Not all systems support pixel_width, pixel_height,
            // but it is good practice to set it to something
            // that matches the size of the selected font.  That
            // is more complex than can be shown here in this
            // brief example though!
            pixel_width: 0,
            pixel_height: 0,
        }).unwrap();

        let mut cmd = CommandBuilder::new("");

        let shell = if cfg!(windows) {
            "powershell.exe".to_string()
        } else {
            cmd.get_shell()
        };

        log::debug!("Using shell '{}'", shell);

        let config_dir = config::get_config_dir();

        if shell.contains("zsh") {
            cmd.args([
                "-il", "+o", "promptsp", "+o", "histignorespace"
            ]);
            cmd.env("ZDOTDIR", config_dir.to_str().unwrap());

            let init = r#"
                # Injected initialization commands
                autoload -Uz add-zsh-hook
                precmd() {
                    CEL_PROMPT_ID=$((CEL_PROMPT_ID + 1))
                    printf '\033]1337;%d\007' "$CEL_PROMPT_ID"
                }
                add-zsh-hook precmd precmd

                # Source the user's original .zshrc if it exists
                if [ -f "$HOME/.zshrc" ]; then
                    source "$HOME/.zshrc"
                fi
            "#;

            // Copy zsh init to config dir
            let _ = std::fs::write(config_dir.join(".zshrc"), init);
        }

        cmd.get_argv_mut()[0] = shell.clone().into();
        //cmd.cwd("/Users/evant/Documents/Projects/cel/test/");
        cmd.env_remove("TERMINFO");
        cmd.env("TERM", "tmux-256color");
        cmd.env("CEL_PROMPT_ID", "0");

        let child = pair.slave.spawn_command(cmd).unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut writer = pair.master.take_writer().unwrap();

        //writer.write_all(b"ls -la\r\n\0");

        if shell.contains("zsh") {
            // Handled above
        } else if shell.contains("powershell") {
            writer.write_all("$global:CEL_PROMPT_ID = 0\r\n".as_bytes());
            writer.write_all("function prompt {\r\n".as_bytes());
            writer.write_all("    $global:CEL_PROMPT_ID++\r\n".as_bytes());
            writer.write_all("    $oscSeq = \"{0}]1337;{1}{2}\" -f [char]0x1b, $global:CEL_PROMPT_ID, [char]0x07\r\n".as_bytes());
            writer.write_all("    Write-Host -NoNewline $oscSeq\r\n".as_bytes());
            writer.write_all("    \"PS \" + (Get-Location) + \"> \"\r\n".as_bytes());
            writer.write_all("}\r\n".as_bytes());
        } else {
            // TODO: fallback mode
            panic!("Shell not supported");
        }

        let (tx, rx) = mpsc::channel();
        let active = Arc::new(AtomicBool::new(true));
        let active_thread = active.clone();
        thread::spawn(move || {
            let mut buf: Vec<u8> = vec![0; 2048];
            loop {
                if !active_thread.load(Ordering::Relaxed) {
                    break;
                }

                // Blocking (pretty sure)
                match reader.read(&mut buf) {
                    Ok(n) => if n > 0 {
                        let _ = tx.send(Vec::from(&buf[..n]));
                    }
                    Err(_) => {}
                }
            }
        });

        Self {
            io_rx: rx,
            pty_pair: pair,
            writer,
            child,
            active,
            output: vec![],
            rows: default_rows as u32,
            cols: default_cols as u32,
        }
    }

    pub fn poll_io(&mut self) {
        while let Ok(v) = self.io_rx.try_recv() {
            self.output.extend(v);
        }
    }

    pub fn resize(&mut self, rows: u32, cols: u32) {
        if (rows == self.rows && cols == self.cols) || rows == 0 || cols == 0 {
            return;
        }

        self.rows = rows;
        self.cols = cols;
        let _ = self.pty_pair.master.resize(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0
        });
    }

    pub fn send_input(&mut self, input: &[u8]) {
        if input.is_empty() {
            return
        }
        let _ = self.writer.write_all(input);
    }

    pub fn get_output(&self) -> &[u8] {
        &self.output
    }

    pub fn clear_output(&mut self) {
        self.output.clear();
    }
}

impl Drop for Commands {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Relaxed);
    }
}
