#[cfg(test)]
mod tests {
    use cel_core::ansi::{AnsiHandler, AnsiBuilder, TerminalState, ScreenBuffer};

    fn setup() -> (AnsiHandler, AnsiBuilder) {
        let mut handler = AnsiHandler::new();
        handler.resize(5, 5);
        let builder = AnsiBuilder::new();

        (handler, builder)
    }

    fn get_final_state(builder: AnsiBuilder) -> TerminalState {
        let (mut handler, _) = setup();
        let stream = builder.build_stream();
        handler.handle_sequence_bytes(&stream, false);

        handler.get_terminal_state().clone()
    }

    fn print_buffer_chars(buf: &Vec<Vec<char>>) -> String {
        let mut res = format!("<len={}>\n", buf.len());
        for (i, line) in buf.iter().enumerate() {
            res.push_str(&format!("[{}] ", line.len()));
            for elem in line {
                res.push_str(&format!("{} ", match elem {
                    '\0' => '.',
                    _ => *elem
                }));
            }
            if i != buf.len() - 1 {
                res.push('\n');
            }
        }

        res
    }

    fn compare_buffer_chars(test: &ScreenBuffer, expect: &Vec<Vec<char>>) -> bool {
        if test.len() != expect.len() {
            return false;
        }

        for i in 0..test.len() {
            if test[i].len() != expect[i].len() {
                return false;
            }

            for j in 0..test[i].len() {
                if test[i][j].elem != expect[i][j] {
                    return false;
                }
            }
        }

        true
    }

    fn assert_buffer_chars_eq(test: &ScreenBuffer, expect: &Vec<Vec<char>>)  {
        let test_str = print_buffer_chars(&test.iter().map(|l| l.iter().map(|e| e.elem).collect()).collect());
        let expect_str = print_buffer_chars(expect);
        assert!(
            test_str == expect_str,
            "Buffers do not match!\nTest: {}\n\n==========\n\nExpect:{}",
            test_str, expect_str
        );
    }

    #[test]
    fn basic() {
        let state = get_final_state(AnsiBuilder::new()
            .add_text("Hello")
            .add_newline()
            .add_text("World")
        );

        let final_buffer = vec![
            vec!['H', 'e', 'l', 'l', 'o'],
            vec!['W', 'o', 'r', 'l', 'd'],
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer)
    }

    #[test]
    fn relative_position_1() {
        let state = get_final_state(AnsiBuilder::new()
            .move_cursor_down(1)
            .move_cursor_right(1)
            .add_text("H")
            .add_newline()
            .add_text("W")
        );

        let final_buffer = vec![
            vec![],
            vec!['.', 'H'],
            vec!['.', '.', 'W'],
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer)
    }

    #[test]
    fn relative_position_2() {
        let state = get_final_state(AnsiBuilder::new()
            .move_cursor_down(1)
            .move_cursor_right(1)
            .add_text("H")
            .add_newline()
            .add_text("W")
        );

        let final_buffer = vec![
            vec![],
            vec!['.', 'H'],
            vec!['.', '.', 'W'],
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer)
    }
}
