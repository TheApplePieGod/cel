#[cfg(test)]
mod tests {
    use cel_core::ansi::AnsiHandler;

    fn setup_handler() -> AnsiHandler {
        let mut handler = AnsiHandler::new();
        handler.resize(5, 5);

        handler
    }

    #[test]
    fn it_works() {
        let mut handler = setup_handler();
        handler.consume_output_stream();
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
