use portable_pty::{CommandBuilder, PtySize, native_pty_system, Child, PtyPair};
use std::{default, sync::mpsc, thread};

#[derive(PartialEq, Eq, Debug)]
enum ShellState {
    Init,
    PromptIdA,
    PromptIdB,
    Ready,
}

pub struct Commands {
    io_rx: mpsc::Receiver<Vec<u8>>,
    //reader: Box<dyn std::io::Read + Send>,
    pty_pair: PtyPair,
    writer: Box<dyn std::io::Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    output: [Vec<u8>; 2],
    rows: u32,
    cols: u32,

    shell_state: ShellState,
    parsing_id: String,
    prompt_id: u32,
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
        let shell = cmd.get_shell();
        if shell.ends_with("zsh") {
            //cmd.args(["-c", &format!("\"{}\"", command)]);
            //cmd.args(["-c", command]);
            //cmd.args(["-is"]);
            //cmd.args(["-i", "-c", "{}; exec {} -i"]);
            cmd.args([
                "-il", "+o", "promptsp", "+o", "histignorespace"
            ]);
        }
        cmd.get_argv_mut()[0] = shell.into();
        //cmd.cwd("/Users/evant/Documents/Projects/cel/test/");
        cmd.env_remove("TERMINFO");
        cmd.env("TERM", "tmux-256color");
        cmd.env("CEL_PROMPT_ID", "0");

        let child = pair.slave.spawn_command(cmd).unwrap();
        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut writer = pair.master.take_writer().unwrap();

        writer.write_all(" PROMPT_COMMAND=$'printf \\\"\\\\x1f\\\\x15$CEL_PROMPT_ID\\\\x15\\\"'\r".as_bytes());
        writer.write_all(" precmd() { CEL_PROMPT_ID=$(($CEL_PROMPT_ID + 1)); eval \"$PROMPT_COMMAND\" }\r".as_bytes());
        writer.write_all(" \r".as_bytes());

        //writer.write_all(b"ls -la\r\n\0");

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf: Vec<u8> = vec![0; 1024];
            loop {
                // Blocking (pretty sure)
                match reader.read(&mut buf) {
                    Ok(n) => if n > 0 {
                        let _ = tx.send(Vec::from(&buf[0..n]));
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
            output: [vec![], vec![]],
            rows: default_rows as u32,
            cols: default_cols as u32,

            shell_state: ShellState::Init,
            parsing_id: String::new(),
            prompt_id: 2,
        }
    }

    // Returns true if the input split while polling
    pub fn poll_io(&mut self) -> bool {
        /* 
        Special state machine for parsing io. Essentially, we give each shell prompt
        a unique id, and the idea is that each command input by the user will be
        uniquely identified by this id. Each prompt starts with a sequence of characters
        and its prompt id, and each command ends with a preprompt with its preprompt id.
        If both of those match the current prompt id we expect, we indicate that the output
        has been split. Otherwise, we assume that there was a redraw, and no splits should
        occur. Anything that occurs inbetween non-current prompts is ignored, which lets
        us inject arbitrary commands into the shell that the user will never see.
        */

        let mut output_idx = 0;
        while let Ok(v) = self.io_rx.try_recv() {
            for byte in v {
                //log::warn!("{:?}", byte as char);
                //self.shell_state = ShellState::Ready;
                match self.shell_state {
                    ShellState::Init | ShellState::Ready if byte == 0x1f
                        => self.shell_state = ShellState::PromptIdA,
                    ShellState::Init
                        => {}
                    ShellState::Ready => {
                        self.output[output_idx].push(byte);
                    },
                    ShellState::PromptIdA if byte == 0x15
                        => self.shell_state = ShellState::PromptIdB,
                    ShellState::PromptIdB if byte == 0x15 => {
                        let parsed_id = self.parsing_id.parse::<u32>();
                        if let Ok(parsed_id) = parsed_id {
                            if parsed_id == self.prompt_id {
                                output_idx = 1;
                                self.set_next_split();
                                self.shell_state = ShellState::Ready;
                            } else {
                                self.shell_state = ShellState::Init;
                            }
                        }
                        self.parsing_id.clear();
                    }
                    ShellState::PromptIdB
                        => self.parsing_id.push(byte as char),
                    _ => {}
                }

                //log::warn!("New state: {:?}", self.shell_state);
            }
        }

        output_idx != 0
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

    pub fn get_output(&self) -> &[Vec<u8>; 2] {
        &self.output
    }

    pub fn clear_output(&mut self) {
        self.output[0].clear();
        self.output[1].clear();
    }

    fn set_next_split(&mut self) {
        self.prompt_id += 1;

        /*
        self.writer.write_all(format!(
            " CEL_PROMPT_ID={}\r",
            self.prompt_id
        ).as_bytes());
        */
    }
}
