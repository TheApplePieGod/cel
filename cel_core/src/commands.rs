use portable_pty::{CommandBuilder, PtySize, native_pty_system, Child, PtyPair};
use std::thread;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, mpsc,
};

use crate::config;

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
    pub fn new(num_rows: u32, num_cols: u32, cwd: Option<&str>) -> Self {
        let pty_system = native_pty_system();

        // Create a new pty
        let pair = pty_system.openpty(PtySize {
            rows: num_rows as u16,
            cols: num_cols as u16,
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
            let og_dir = std::env::var("ZDOTDIR").unwrap_or("$HOME".to_string());
            cmd.args([
                "-il", "+o", "promptsp", "+o", "histignorespace"
            ]);
            cmd.env("ZDOTDIR", config_dir.to_str().unwrap());

            let init = r#"
                autoload -Uz add-zsh-hook
                precmd() {
                    printf '\033]1339;%s\007' "$?"
                    CEL_PROMPT_ID=$((${CEL_PROMPT_ID:-0} + 1))
                    printf '\033]1337;%d\007' "$CEL_PROMPT_ID"
                    printf '\033]1338;%s\007' "$(pwd)"
                    # Scuffed, but guarantees that things don't get messed up
                    TERM=tmux-256color
                }
                add-zsh-hook precmd precmd

                # Map alt+left/right to move by word if not already mapped
                [[ $(builtin bindkey "^[[1;3C") == *" undefined-key" ]] && builtin bindkey "^[[1;3C" "forward-word"
                [[ $(builtin bindkey "^[[1;3D") == *" undefined-key" ]] && builtin bindkey "^[[1;3D" "backward-word"

                # Enable UTF8
                export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
                # Indicate integration
                export CEL_SHELL_INTEGRATION=1
                # Reset ZDOTDIR
                export ZDOTDIR="$OG_DIR"

                # Source the user's original .zshxx files if they exist
                if [ -f "$OG_DIR/.zshenv" ]; then
                    source "$OG_DIR/.zshenv"
                fi
                if [ -f "$OG_DIR/.zprofile" ]; then
                    source "$OG_DIR/.zprofile"
                fi
                if [ -f "$OG_DIR/.zshrc" ]; then
                    source "$OG_DIR/.zshrc"
                fi
                if [ -f "$OG_DIR/.zlogin" ]; then
                    source "$OG_DIR/.zlogin"
                fi
            "#.replace("$OG_DIR", &og_dir);

            // Copy zsh init to config dir
            let _ = std::fs::write(config_dir.join(".zshrc"), init);
        } else if shell.contains("bash") {
            cmd.args(["--login", "-i"]);

            let init = r#"
                precmd() {
                    printf '\033]1339;%s\007' "$?"
                    CEL_PROMPT_ID=$(( ${CEL_PROMPT_ID:-0} + 1 ))
                    printf '\033]1337;%d\007' "$CEL_PROMPT_ID"
                    printf '\033]1338;%s\007' "$(pwd)"
                    CEL_LAST_PWD=$(pwd)
                    # Scuffed, but guarantees that things don't get messed up
                    TERM=tmux-256color
                }
                PROMPT_COMMAND="precmd"

                bind '"\e[1;3C": forward-word'
                bind '"\e[1;3D": backward-word'

                export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
            "#;

            // Write the bash init file to the custom config directory (e.g., as ".bashrc").
            let _ = std::fs::write(config_dir.join(".bashrc"), init);
        }

        cmd.get_argv_mut()[0] = shell.clone().into();
        cmd.env_remove("TERMINFO");
        cmd.env("TERM", "tmux-256color");
        cmd.env("CEL_PROMPT_ID", "0");
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }

        let child = pair.slave.spawn_command(cmd).unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut writer = pair.master.take_writer().unwrap();

        //writer.write_all(b"ls -la\r\n\0");

        // Extra init commands
        if shell.contains("bash") {
            let _ = writer.write_all(format!(" source '{}'\n", config_dir.join(".bashrc").to_str().unwrap()).as_bytes());
        } else if shell.contains("powershell") {
            let _ = writer.write_all("$global:CEL_PROMPT_ID = 0\r\n".as_bytes());
            let _ = writer.write_all("function prompt {\r\n".as_bytes());
            let _ = writer.write_all("    $global:CEL_PROMPT_ID++\r\n".as_bytes());
            let _ = writer.write_all("    $oscSeq = \"{0}]1337;{1}{2}\" -f [char]0x1b, $global:CEL_PROMPT_ID, [char]0x07\r\n".as_bytes());
            let _ = writer.write_all("    Write-Host -NoNewline $oscSeq\r\n".as_bytes());
            let _ = writer.write_all("    \"PS \" + (Get-Location) + \"> \"\r\n".as_bytes());
            let _ = writer.write_all("}\r\n".as_bytes());
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
            rows: num_rows,
            cols: num_cols,
        }
    }

    // Returns true if the commands have terminated
    pub fn poll_io(&mut self) -> bool {
        while let Ok(v) = self.io_rx.try_recv() {
            self.output.extend(v);
        }

        let wait_result = self.child.try_wait();
        wait_result.is_err() || wait_result.unwrap().is_some()
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
