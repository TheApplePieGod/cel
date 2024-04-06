mod common;

#[cfg(test)]
mod tests {
    use cel_core::ansi::AnsiBuilder;

    use crate::common::{get_final_state, assert_buffer_chars_eq};

    // TODO: tests for insert, delete lines & chars
    // TODO: tests for scroll region

    #[test]
    fn basic() {
        let state = get_final_state(AnsiBuilder::new()
            .add_text("Hello")
            .add_newline_and_cr()
            .add_text("World")
        );

        let final_buffer = vec![
            vec!['H', 'e', 'l', 'l', 'o'],
            vec!['W', 'o', 'r', 'l', 'd'],
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer);
        assert!(state.wants_wrap);
    }
    
    #[test]
    fn absolute_position_1() {
        let state = get_final_state(AnsiBuilder::new()
            .add_text("12345")
            .add_text("67890")
            .add_text("12345")
            .add_newline_and_cr()
            .add_text("ABCDE")
            .set_cursor_pos(2, 3)
            .add_text("X")
        );

        let final_buffer = vec![
            vec!['1', '2', '3', '4', '5',
                 '6', '7', '8', '9', '0',
                 '1', 'X', '3', '4', '5'],
            vec!['A', 'B', 'C', 'D', 'E'],
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer);
        assert!(!state.wants_wrap);
    }

    #[test]
    fn absolute_position_2() {
        let state = get_final_state(AnsiBuilder::new()
            .add_text("12345")
            .set_cursor_pos(1, 2)
            .add_text("67890")
            .set_cursor_pos(1, 1)
            .add_text("X")
        );

        let final_buffer = vec![
            vec!['X', '2', '3', '4', '5'],
            vec!['6', '7', '8', '9', '0']
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer);
        assert!(!state.wants_wrap);
    }

    #[test]
    fn absolute_position_3() {
        let state = get_final_state(AnsiBuilder::new()
            .add_text("12345")
            .set_cursor_pos(1, 1)
        );

        let final_buffer = vec![
            vec!['1', '2', '3', '4', '5'],
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer);
        assert!(!state.wants_wrap);
    }

    #[test]
    fn relative_position_1() {
        let state = get_final_state(AnsiBuilder::new()
            .move_cursor_down(1)
            .move_cursor_right(1)
            .add_text("1")
            .add_newline()
            .add_text("2")
            .move_cursor_left(10)
            .add_text("3")
            .move_cursor_up(10)
            .add_text("4")
        );

        let final_buffer = vec![
            vec!['.', '4'],
            vec!['.', '1',],
            vec!['3', '.', '2'],
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer);
        assert!(!state.wants_wrap);
    }

    #[test]
    fn relative_position_2() {
        let state = get_final_state(AnsiBuilder::new()
            .add_text("12345")
            .add_text("67890")
            .move_cursor_up(1)
            .move_cursor_right(1)
            .add_text("H")
            .move_cursor_down(1)
            .move_cursor_left(1)
            .add_text("W")
        );

        let final_buffer = vec![
            vec!['1', '2', '3', '4', 'H',
                 '6', '7', '8', 'W', '0']
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer);
        assert!(!state.wants_wrap);
    }

    #[test]
    fn newline_1() {
        let state = get_final_state(AnsiBuilder::new()
            .add_text("12345")
            .add_text("67")
            .move_cursor_up(1)
            .move_cursor_right(100)
            .add_newline()
            .add_text("H")
        );

        let final_buffer = vec![
            vec!['1', '2', '3', '4', '5',
                 '6', '7', '.', '.', 'H']
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer);
        assert!(state.wants_wrap);
    }

    #[test]
    fn carriage_return_1() {
        let state = get_final_state(AnsiBuilder::new()
            .add_text("12345")
            .add_text("67")
            .move_cursor_up(1)
            .move_cursor_right(100)
            .add_newline_and_cr()
            .add_text("H")
        );

        let final_buffer = vec![
            vec!['1', '2', '3', '4', '5',
                 'H', '7']
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer);
        assert!(!state.wants_wrap);
    }

    #[test]
    fn carriage_return_2() {
        let state = get_final_state(AnsiBuilder::new()
            .add_text("12345")
            .add_newline_and_cr()
            .add_text("H")
        );

        let final_buffer = vec![
            vec!['1', '2', '3', '4', '5'],
            vec!['H']
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer);
        assert!(!state.wants_wrap);
    }

    #[test]
    fn home_cursor_1() {
        let state = get_final_state(AnsiBuilder::new()
            .add_text("12345")
            .set_cursor_pos(1, 2)
            .add_text("12345")
            .set_cursor_pos(1, 3)
            .add_text("12345")
            .set_cursor_pos(1, 4)
            .add_text("12345")
            .set_cursor_pos(1, 5)
            .add_text("12345")
            .set_cursor_pos(1, 1)
            .add_text("H")
        );

        let final_buffer = vec![
            vec!['H', '2', '3', '4', '5'],
            vec!['1', '2', '3', '4', '5'],
            vec!['1', '2', '3', '4', '5'],
            vec!['1', '2', '3', '4', '5'],
            vec!['1', '2', '3', '4', '5']
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer);
        assert!(!state.wants_wrap);
    }

    #[test]
    fn scroll_region_1() {
        let state = get_final_state(AnsiBuilder::new()
            .add_text("1234567890123") // 3 lines
            .add_newline_and_cr()
            .add_text("1234567890123") // 3 lines
        );

        let final_buffer = vec![
            vec!['1', '2', '3', '4', '5',
                 '6', '7', '8', '9', '0',
                 '1', '2', '3',],
            vec!['1', '2', '3', '4', '5',
                 '6', '7', '8', '9', '0',
                 '1', '2', '3']
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer);
        assert_eq!(state.global_cursor_home, [5, 0]);
        assert!(!state.wants_wrap);
    }

    #[test]
    fn scroll_region_2() {
        let state = get_final_state(AnsiBuilder::new()
            .set_scroll_margin_y(2, 4)
            .add_text("1234567890123") // 3 lines
            .add_newline_and_cr()
            .add_text("1234567890123") // 3 lines
        );

        // Note: these lines *should* stay wrapped, but the current
        // implementation recomputes the cursor and thus resets the wrap
        // state. If this changes in the future, now you know why.

        let final_buffer = vec![
            vec!['1', '2', '3', '4', '5'],
            vec!['1', '2', '3', '4', '5'],
            vec!['6', '7', '8', '9', '0'],
            vec!['1', '2', '3'],
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer);
        assert_eq!(state.margin.top, 1);
        assert_eq!(state.margin.bottom, 3);
        assert_eq!(state.global_cursor_home, [0, 0]);
        assert!(!state.wants_wrap);
    }

    #[test]
    fn scroll_region_3() {
        let state = get_final_state(AnsiBuilder::new()
            .add_text("1234567890123") // 3 lines
            .add_newline_and_cr()
            .add_text("1234567890SAFE") // 3 lines
            .set_scroll_margin_y(2, 4)
            .set_cursor_pos(1, 4)
            .remove_lines(2)
            .set_cursor_pos(1, 2)
            .insert_lines(2)
        );

        let final_buffer = vec![
            vec!['1', '2', '3', '4', '5',
                 '6', '7', '8', '9', '0'],
            vec![],
            vec![],
            vec!['1', '2', '3',],
            vec!['S', 'A', 'F', 'E'],
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer);
        assert_eq!(state.margin.top, 1);
        assert_eq!(state.margin.bottom, 3);
        assert_eq!(state.global_cursor_home, [5, 0]);
        assert!(!state.wants_wrap);
    }

    #[test]
    fn reverse_index_1() {
        let state = get_final_state(AnsiBuilder::new()
            .add_text("1234567890123") // 3 lines
            .add_newline_and_cr()
            .add_text("1234567890123") // 3 lines
            .reverse_index()
            .set_cursor_pos(1, 1)
            .add_text("A")
        );

        let final_buffer = vec![
            vec!['1', '2', '3', '4', '5',
                 'A', '7', '8', '9', '0',
                 '1', '2', '3'],
            vec!['1', '2', '3', '4', '5',
                 '6', '7', '8', '9', '0',
                 '1', '2', '3'],
        ];
        assert_buffer_chars_eq(&state.screen_buffer, &final_buffer);
        assert_eq!(state.global_cursor_home, [5, 0]);
        assert_eq!(state.screen_cursor, [1, 0]);
        assert!(!state.wants_wrap);
    }
}
