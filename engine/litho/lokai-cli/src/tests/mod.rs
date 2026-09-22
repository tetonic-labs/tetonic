//! CLI unit tests.

mod args_tests {
    use clap::Parser;

    use crate::args::Args;

    #[test]
    fn omitted_model_is_distinct_from_explicit_default() {
        assert!(Args::try_parse_from(["lokai"]).unwrap().model.is_none());
        assert_eq!(
            Args::try_parse_from(["lokai", "--model", tetonic_app::DEFAULT_MODEL])
                .unwrap()
                .model
                .as_deref(),
            Some(tetonic_app::DEFAULT_MODEL)
        );
    }

    #[test]
    fn explain_flag_parses() {
        let args = Args::try_parse_from(["lokai", "--explain", "what is this repo"]).unwrap();
        assert!(args.explain);
        assert_eq!(args.prompt.join(" "), "what is this repo");
    }

    #[test]
    fn orchestrate_auto_parses() {
        let args =
            Args::try_parse_from(["lokai", "--orchestrate", "auto", "implement foo"]).unwrap();
        assert_eq!(args.orchestrate, "auto");
    }

    #[test]
    fn debug_flag_parses() {
        let args = Args::try_parse_from(["lokai", "--debug"]).unwrap();
        assert!(args.debug);
        let args_short = Args::try_parse_from(["lokai", "-d"]).unwrap();
        assert!(args_short.debug);
        let args_default = Args::try_parse_from(["lokai"]).unwrap();
        assert!(!args_default.debug);
    }
}

mod explain_tests {
    use tetonic_app::definition::CodingAgentDefinition;

    #[test]
    fn dogfood_summary_is_explain_only() {
        let q = "hey can you tell me a brief summary about the lokai. i dont really understand what it does";
        assert!(CodingAgentDefinition::production().root_explain_turn(q));
    }

    #[test]
    fn implement_task_is_not_explain_only() {
        assert!(!CodingAgentDefinition::production()
            .root_explain_turn("Implement greeting tweak in a.txt"));
    }
}
